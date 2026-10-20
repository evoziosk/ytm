use super::{
    clock::Clock,
    sync::{decide, host_frame},
};
use crate::{
    commands::dispatch,
    state::{now_ms, Core, PartyState},
};
use futures_util::{SinkExt, StreamExt};
use rand::Rng;
use std::sync::atomic::Ordering;
use tauri::AppHandle;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::{protocol::WebSocketConfig, Message};
use ytm_protocol::{PartyMsg, PlayerCommand, Role, SyncFrame, VERSION};

pub fn room_code() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut rng = rand::rng();
    (0..8)
        .map(|_| ALPHABET[rng.random_range(0..ALPHABET.len())] as char)
        .collect()
}
pub fn leave(app: &AppHandle, core: &Core) {
    core.party_generation.fetch_add(1, Ordering::SeqCst);
    if let Some(stop) = core.party_stop.lock().expect("party stop").take() {
        let _ = stop.send(());
    }
    core.party_out.lock().expect("party sender").take();
    core.data.write().expect("state lock").party = PartyState::default();
    let _ = dispatch(app, &PlayerCommand::SetLocked { locked: false });
    let _ = dispatch(app, &PlayerCommand::Rate { rate: 1.0 });
    core.emit(app);
}
pub fn start(app: &AppHandle, core: &Core, input: &str, hosting: bool) -> Result<(), String> {
    if crate::control_client::active(core) {
        return Err("Disconnect from the remote host before joining a room".into());
    }
    if core.control.lock().expect("control lock").session.is_some() {
        return Err("End remote/group control before joining a listening room".into());
    }
    let settings = core.snapshot().settings;
    if settings.relay_url.is_empty() {
        return Err("Set your relay URL in Settings → Listen together first".into());
    }
    let code = if hosting {
        room_code()
    } else {
        input.trim().to_uppercase()
    };
    if !ytm_protocol::valid_room(&code) {
        return Err("Room codes contain 6–8 unambiguous letters and numbers".into());
    }
    let mut url = url::Url::parse(&settings.relay_url).map_err(|e| e.to_string())?;
    let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
    url.set_scheme(scheme).map_err(|_| "Invalid relay scheme")?;
    url.set_path(&format!("/room/{code}"));
    let token = {
        let mut rng = rand::rng();
        (0..32)
            .map(|_| format!("{:02x}", rng.random::<u8>()))
            .collect::<String>()
    };
    let (stop_tx, stop_rx) = oneshot::channel();
    let (out_tx, out_rx) = mpsc::channel(16);
    let mut stop_slot = core.party_stop.lock().expect("party stop");
    if stop_slot.is_some() {
        return Err("Leave the current room before starting another".into());
    }
    let generation = core.party_generation.fetch_add(1, Ordering::SeqCst) + 1;
    *stop_slot = Some(stop_tx);
    drop(stop_slot);
    *core.party_out.lock().expect("party sender") = Some(out_tx);
    {
        let mut data = core.data.write().expect("state lock");
        data.pending_room = None;
        if !hosting {
            data.queue.clear();
            data.queue_revision = data.queue_revision.wrapping_add(1);
            data.queue_playlist_id.clear();
            data.previous_playlist = None;
            data.queue_index = 0;
        }
        data.party = PartyState {
            code: code.clone(),
            reconnecting: true,
            message: "Connecting…".into(),
            invite_url: format!("{}/join/{code}", settings.relay_url),
            ..Default::default()
        };
    }
    core.emit(app);
    let app = app.clone();
    let core = core.clone();
    tauri::async_runtime::spawn(async move {
        tokio::select! {
            _ = stop_rx => (),
            _ = run(&app, &core, url.as_str(), &settings.display_name, &token, hosting, out_rx) => {
                if core.party_generation.load(Ordering::SeqCst) == generation {
                    core.party_stop.lock().expect("party stop").take();
                    core.party_out.lock().expect("party sender").take();
                    let mut data = core.data.write().expect("state lock"); data.party.role = None; data.party.connected = false; data.party.reconnecting = false;
                }
            }
        }
        if core.party_generation.load(Ordering::SeqCst) == generation {
            let _ = dispatch(&app, &PlayerCommand::SetLocked { locked: false });
            let _ = dispatch(&app, &PlayerCommand::Rate { rate: 1.0 });
            core.emit(&app);
        }
    });
    Ok(())
}
async fn run(
    app: &AppHandle,
    core: &Core,
    url: &str,
    name: &str,
    token: &str,
    hosting: bool,
    mut out: mpsc::Receiver<PartyMsg>,
) {
    let mut hosting = hosting;
    let mut backoff = 1_u64;
    let mut sequence = 0_u32;
    loop {
        // Bound handshake and incoming frames. Application messages are capped again after decoding.
        let config = WebSocketConfig::default()
            .max_message_size(Some(32768))
            .max_frame_size(Some(32768));
        let connection = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            tokio_tungstenite::connect_async_with_config(url, Some(config), false),
        )
        .await;
        if let Ok(Ok((mut socket, _))) = connection {
            let hello = PartyMsg::Hello {
                name: name.into(),
                version: VERSION,
                token: token.into(),
                hosting,
            };
            if send(&mut socket, &hello).await.is_ok() {
                let mut changes = core.changes.subscribe();
                let mut clock = Clock::default();
                let mut frame: Option<SyncFrame> = None;
                let mut role: Option<Role> = None;
                let mut member_id = String::new();
                let mut tick = tokio::time::interval(std::time::Duration::from_millis(500));
                let mut last_ping = 0.0;
                let mut ping_burst = 0_u32;
                let mut pending_pings = Vec::<f64>::new();
                let mut last_sync = 0.0;
                let mut last_status = 0.0;
                let mut last_load = 0.0;
                let mut last_host_frame: Option<SyncFrame> = None;
                let mut transferring = false;
                let mut host_away = false;
                let mut welcome_deadline = now_ms() + 10000.0;
                let mut last_received = now_ms();
                loop {
                    tokio::select! {
                        incoming = socket.next() => {
                            let Some(Ok(incoming)) = incoming else { break; };
                            last_received = now_ms();
                            match incoming {
                                Message::Text(text) => {
                                    let Ok(message) = serde_json::from_str::<PartyMsg>(&text) else { break; };
                                    match message {
                                        PartyMsg::Welcome { id, role: new_role, members, last_sync: replay } => {
                                            if new_role == Role::Member { hosting = false; }
                                            backoff = 1; member_id = id; role = Some(new_role.clone());
                                            sequence = replay.as_ref().map_or(sequence, |f| sequence.max(f.seq)); frame = replay;
                                            { let mut d = core.data.write().expect("state lock"); d.party.role = role.clone(); d.party.members = members; d.party.connected = true; d.party.reconnecting = false; d.party.message = "Connected".into(); }
                                            let _ = dispatch(app, &PlayerCommand::SetLocked { locked: new_role == Role::Member }); core.emit(app);
                                        }
                                        PartyMsg::Pong { t0, server_t } => {
                                            if let Some(i) = pending_pings.iter().position(|v| *v == t0) { pending_pings.swap_remove(i); clock.sample(t0, now_ms(), server_t); }
                                        }
                                        PartyMsg::Sync { frame: next } => {
                                            if frame.as_ref().is_none_or(|old| next.seq > old.seq) { host_away = false; frame = Some(next); }
                                        }
                                        PartyMsg::Members { members } => {
                                            transferring = false;
                                            if let Some(me) = members.iter().find(|m| m.id == member_id) {
                                                if me.role == Role::Member { hosting = false; }
                                                if role.as_ref() != Some(&me.role) { role = Some(me.role.clone()); frame = None; last_host_frame = None; last_sync = 0.0; let _ = dispatch(app, &PlayerCommand::SetLocked { locked: me.role == Role::Member }); }
                                            }
                                            { let mut d = core.data.write().expect("state lock"); d.party.members = members; d.party.role = role.clone(); } core.emit(app);
                                        }
                                        PartyMsg::HostAway => { host_away = true; let _ = dispatch(app, &PlayerCommand::Pause); core.data.write().expect("state lock").party.message = "Host disconnected. Waiting for them to return…".into(); core.emit(app); }
                                        PartyMsg::Error { message } => { core.data.write().expect("state lock").party.message = message; return; }
                                        _ => (),
                                    }
                                }
                                Message::Ping(bytes) => { if socket.send(Message::Pong(bytes)).await.is_err() { break; } }
                                Message::Close(close) => {
                                    if close.as_ref().is_some_and(|c| matches!(u16::from(c.code), 1000 | 1008)) { core.data.write().expect("state lock").party.message = close.map(|c| c.reason.to_string()).unwrap_or_else(|| "Room closed".into()); return; }
                                    break;
                                }
                                _ => (),
                            }
                        }
                        command = out.recv() => {
                            if let Some(command) = command {
                                if matches!(command, PartyMsg::HostTransfer { .. }) { transferring = true; }
                                if send(&mut socket, &command).await.is_err() { break; }
                            }
                        }
                        _ = changes.recv() => { /* The 500 ms tick coalesces changes without flooding the relay. */ }
                        _ = tick.tick() => {
                            let now = now_ms();
                            if now - last_received > 45000.0 { break; }
                            if role.is_none() && now > welcome_deadline { break; }
                            if role.is_some() { welcome_deadline = f64::INFINITY; }
                            if now - last_ping > if ping_burst < 5 { 500.0 } else { 30000.0 } {
                                if ping_burst >= 5 { ping_burst = 0; }
                                last_ping = now; ping_burst += 1; pending_pings.retain(|t| now - t < 10000.0); pending_pings.push(now);
                                if send(&mut socket, &PartyMsg::Ping { t0: now }).await.is_err() { break; }
                            }
                            let state = core.snapshot().player;
                            if role == Some(Role::Host) && !transferring {
                                if let Some(mut next) = host_frame(&state, last_host_frame.as_ref(), now, clock.offset()) {
                                    let changed = last_host_frame.as_ref().is_none_or(|last| {
                                        let expected = last.position + if last.playing { (now + clock.offset() - last.host_ts) / 1000.0 } else { 0.0 };
                                        next.video_id != last.video_id || next.playing != last.playing || (next.position - expected).abs() > 1.0
                                    });
                                    if (changed || now - last_sync >= 5000.0) && clock.ready() {
                                        last_sync = now; sequence = sequence.wrapping_add(1);
                                        next.seq = sequence;
                                        if send(&mut socket, &PartyMsg::Sync { frame: next.clone() }).await.is_err() { break; }
                                        last_host_frame = Some(next);
                                    }
                                }
                            } else if role == Some(Role::Member) && !host_away && clock.ready() {
                                // Reapply after page navigation/reload, which replaces the injected bridge.
                                let _ = dispatch(app, &PlayerCommand::SetLocked { locked: true });
                                if let Some(frame) = &frame {
                                    let decision = decide(&state, frame, now, clock.offset());
                                    for command in decision.commands {
                                        if matches!(command, PlayerCommand::LoadTrack { .. }) { if now - last_load < 5000.0 { continue; } last_load = now; }
                                        let _ = dispatch(app, &command);
                                    }
                                    core.data.write().expect("state lock").party.drift_ms = decision.drift_ms;
                                    if now - last_status >= 5000.0 { last_status = now; if send(&mut socket, &PartyMsg::Status { state: decision.status, drift_ms: decision.drift_ms }).await.is_err() { break; } core.emit(app); }
                                }
                            }
                        }
                    }
                }
            }
        }
        {
            let mut d = core.data.write().expect("state lock");
            d.party.connected = false;
            d.party.reconnecting = true;
            d.party.message = format!("Connection lost. Retrying in {backoff}s…");
        }
        if core.snapshot().party.role == Some(Role::Member) {
            let _ = dispatch(app, &PlayerCommand::Pause);
            let _ = dispatch(app, &PlayerCommand::Rate { rate: 1.0 });
        }
        core.emit(app);
        tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
        backoff = (backoff * 2).min(30);
    }
}
async fn send<S>(socket: &mut S, msg: &PartyMsg) -> Result<(), ()>
where
    S: futures_util::Sink<Message> + Unpin,
{
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        socket.send(Message::Text(
            serde_json::to_string(msg).map_err(|_| ())?.into(),
        )),
    )
    .await
    .map_err(|_| ())?
    .map_err(|_| ())
}
