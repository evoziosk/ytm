use crate::state::Core;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_store::StoreExt;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::{protocol::WebSocketConfig, Message};
use ytm_protocol::*;

type Outgoing = (
    ControlRequest,
    oneshot::Sender<Result<ControlResult, String>>,
);
#[derive(Clone, Default, Serialize)]
pub struct ClientStatus {
    pub active: bool,
    pub target: String,
    pub message: String,
    pub participant: Option<ControlParticipant>,
    pub snapshot: Option<RemoteSnapshot>,
    pub updated_at: f64,
    pub devices: Vec<SavedDevice>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct SavedDevice {
    pub name: String,
    pub address: String,
}
#[derive(Default)]
pub struct ClientState {
    status: ClientStatus,
    generation: String,
    stop: Option<oneshot::Sender<()>>,
    out: Option<mpsc::Sender<Outgoing>>,
}
pub fn active(core: &Core) -> bool {
    core.remote.lock().expect("remote lock").status.active
}
pub fn status(core: &Core) -> ClientStatus {
    core.remote.lock().expect("remote lock").status.clone()
}
pub fn queue_revision(core: &Core) -> Option<u32> {
    core.remote
        .lock()
        .expect("remote lock")
        .status
        .snapshot
        .as_ref()
        .map(|s| s.queue_revision)
}
#[tauri::command]
pub fn remote_status(app: AppHandle, core: State<Core>) -> ClientStatus {
    let mut status = core.remote.lock().expect("remote lock").status.clone();
    status.devices = saved_devices(&app);
    status
}
fn saved_devices(app: &AppHandle) -> Vec<SavedDevice> {
    app.store(format!("control-{}.json", crate::profile()))
        .ok()
        .and_then(|store| store.get("devices"))
        .and_then(|value| serde_json::from_value::<Vec<SavedDevice>>(value).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|d| {
            d.name.len() <= 512
                && parse_address(&d.address)
                    .is_ok_and(|u| local_host(&u) && u.path() == "/" && u.fragment().is_none())
        })
        .take(20)
        .collect()
}
fn remember_device(app: &AppHandle, name: &str, socket: &str) {
    let Ok(mut url) = url::Url::parse(socket) else {
        return;
    };
    if !local_host(&url) || url.path() != "/ws" {
        return;
    }
    let scheme = if url.scheme() == "wss" {
        "https"
    } else {
        "http"
    };
    if url.set_scheme(scheme).is_err() {
        return;
    }
    url.set_path("/");
    let device = SavedDevice {
        name: name.into(),
        address: url.to_string(),
    };
    let mut devices = saved_devices(app);
    if devices.first() == Some(&device) {
        return;
    }
    devices.retain(|d| d.address != device.address);
    devices.insert(0, device);
    devices.truncate(20);
    if let Ok(store) = app.store(format!("control-{}.json", crate::profile())) {
        store.set("devices", serde_json::to_value(devices).expect("devices"));
        if let Err(error) = store.save() {
            tracing::warn!(%error, "Could not remember playback device");
        }
    }
}
#[tauri::command]
pub async fn remote_connect(
    app: AppHandle,
    core: State<'_, Core>,
    invitation: String,
) -> Result<(), String> {
    let _guard = core.mutation.lock().await;
    if active(&core)
        || core.control.lock().expect("control lock").session.is_some()
        || core.party_stop.lock().expect("party lock").is_some()
    {
        return Err("Leave the current shared playback mode before connecting".into());
    }
    let (mut url, session_id, invite, mode) = resolve_invitation(&invitation).await?;
    let origin = url.origin().ascii_serialization();
    let settings = core.snapshot().settings;
    let store = app
        .store(format!("control-{}.json", crate::profile()))
        .map_err(|e| e.to_string())?;
    let key = format!("remote-{}", ytm_control::digest(&origin));
    let credential = if mode == ControlMode::Personal {
        let credential = store
            .get(&key)
            .and_then(|v| v.as_str().map(String::from))
            .filter(|s| ytm_control::valid_secret(s))
            .unwrap_or_else(ytm_control::secret);
        store.set(&key, credential.clone());
        store.save().map_err(|e| e.to_string())?;
        credential
    } else {
        ytm_control::secret()
    };
    let relay = url.path() == "/controller/";
    let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
    url.set_scheme(scheme).map_err(|_| "Invalid scheme")?;
    url.set_path(&if relay {
        format!("/control/{session_id}")
    } else {
        "/ws".into()
    });
    url.set_fragment(None);
    if core.snapshot().player.playing {
        crate::commands::perform_serialized(&app, PlayerCommand::Pause).await?;
    }
    let (out, receiver) = mpsc::channel(32);
    let (stop, stopped) = oneshot::channel();
    let generation = ytm_control::secret();
    *core.remote.lock().expect("remote lock") = ClientState {
        status: ClientStatus {
            active: true,
            target: origin,
            message: "Connecting".into(),
            ..Default::default()
        },
        generation: generation.clone(),
        stop: Some(stop),
        out: Some(out),
    };
    let hello = ControlClientMsg::Hello {
        version: CONTROL_VERSION,
        session_id,
        invitation: invite,
        credential,
        name: settings.display_name,
        mode,
    };
    publish(&app);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::select! { _ = stopped => (), _ = run(&app, url.as_str(), hello, &generation, receiver) => () }
    });
    Ok(())
}
fn local_host(url: &url::Url) -> bool {
    url.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .parse::<std::net::Ipv4Addr>()
                .is_ok_and(|ip| ip.is_private() || ip.is_loopback() || ip.is_link_local())
    })
}
fn parse_address(input: &str) -> Result<url::Url, String> {
    let input = input.trim();
    if input.is_empty() || input.len() > 4096 {
        return Err("Enter the host's LAN address or full invitation link".into());
    }
    let bare = !input.contains("://");
    let mut url = url::Url::parse(&if bare {
        format!("http://{input}")
    } else {
        input.to_owned()
    })
    .map_err(|_| "Invalid host address or invitation link")?;
    if !(url.scheme() == "https" || url.scheme() == "http" && local_host(&url))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || !matches!(url.path(), "/" | "/controller/")
    {
        return Err(
            "Use a local IP address with port 9863, or a full HTTPS relay invitation".into(),
        );
    }
    if bare && local_host(&url) && url.port().is_none() {
        url.set_port(Some(9863)).map_err(|_| "Invalid host port")?;
    }
    Ok(url)
}
pub fn parse_link(input: &str) -> Result<(url::Url, String, String, ControlMode), String> {
    let url = parse_address(input)?;
    let values: HashMap<_, _> =
        url::form_urlencoded::parse(url.fragment().unwrap_or("").as_bytes())
            .into_owned()
            .collect();
    let session = values.get("session").filter(|s| ytm_control::valid_secret(s)).ok_or("The invitation is incomplete. Use Copy invitation link on the host, or enter only its LAN IP address and port")?.clone();
    let invite = values
        .get("invite")
        .filter(|s| ytm_control::valid_secret(s))
        .ok_or("The invitation is incomplete. Copy the full invitation link from the host")?
        .clone();
    let mode = match values.get("mode").map(String::as_str) {
        Some("personal") => ControlMode::Personal,
        Some("group") => ControlMode::Group,
        _ => {
            return Err("Invalid sharing mode in invitation; copy a new link from the host".into())
        }
    };
    Ok((url, session, invite, mode))
}
pub(crate) async fn resolve_invitation(
    input: &str,
) -> Result<(url::Url, String, String, ControlMode), String> {
    let url = parse_address(input)?;
    if url.fragment().is_some_and(|f| !f.is_empty()) {
        return parse_link(input);
    }
    if !local_host(&url) || url.path() != "/" {
        return Err(
            "Relay connections need the full invitation link, including #session=...".into(),
        );
    }
    let endpoint = url.join("session").map_err(|_| "Invalid host address")?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let mut response = client.get(endpoint).send().await.map_err(|_| {
        "Could not reach the host. Check its address, sharing status, network and firewall"
    })?;
    if !response.status().is_success() {
        return Err("The host does not support address-only joining, or sharing is disabled. Update the host app, or use its full invitation link".into());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Could not read the host session")?
    {
        if body.len() + chunk.len() > 8192 {
            return Err("Invalid host session response".into());
        }
        body.extend_from_slice(&chunk);
    }
    let session: ControlDiscovery = serde_json::from_slice(&body).map_err(|_| {
        "This address is not a YTM Listen host. Update the host app or use its full invitation link"
    })?;
    if session.version != CONTROL_VERSION
        || !ytm_control::valid_secret(&session.session_id)
        || session.host_name.len() > 64
    {
        return Err("Invalid or incompatible host session".into());
    }
    Ok((url, session.session_id, String::new(), session.mode))
}
#[tauri::command]
pub async fn remote_disconnect(app: AppHandle, core: State<'_, Core>) -> Result<(), String> {
    let mut state = core.remote.lock().expect("remote lock");
    if let Some(stop) = state.stop.take() {
        let _ = stop.send(());
    }
    *state = ClientState::default();
    drop(state);
    publish(&app);
    Ok(())
}
#[tauri::command]
pub async fn remote_request(
    core: State<'_, Core>,
    command: ControlCommand,
) -> Result<ControlResult, String> {
    request(&core, command).await
}
pub async fn request(core: &Core, command: ControlCommand) -> Result<ControlResult, String> {
    let (sender, receiver) = oneshot::channel();
    {
        let state = core.remote.lock().expect("remote lock");
        if !state.status.active
            || state.status.message != "Connected"
            || !state
                .status
                .participant
                .as_ref()
                .is_some_and(|p| p.approved)
        {
            return Err("Waiting for host connection and approval".into());
        }
        state
            .out
            .as_ref()
            .ok_or("Disconnected")?
            .try_send((
                ControlRequest {
                    id: ytm_control::secret()[..32].into(),
                    command,
                },
                sender,
            ))
            .map_err(|_| "Too many remote commands")?;
    }
    tokio::time::timeout(std::time::Duration::from_secs(30), receiver)
        .await
        .map_err(|_| "No acknowledgement; command will not be replayed")?
        .map_err(|_| "Disconnected; uncertain commands are not replayed")?
}
pub async fn playback(core: &Core, command: PlayerCommand) -> Result<(), String> {
    let remote = {
        let state = core.remote.lock().expect("remote lock");
        playback_command(&state.status, command)?
    };
    request(core, ControlCommand::Playback { command: remote }).await?;
    Ok(())
}
pub async fn catalog(core: &Core, query: CatalogRequest) -> Result<CatalogPage, String> {
    let offset = if query.continuation.is_empty() {
        0
    } else {
        query
            .continuation
            .strip_prefix("remote:")
            .and_then(|s| s.parse::<u32>().ok())
            .filter(|n| *n <= 500)
            .ok_or("Invalid catalog page")?
    };
    let command = match query.kind {
        CatalogKind::Search | CatalogKind::PublicSearch => ControlCommand::Search { query: query.query, offset, limit: 50 },
        CatalogKind::Browse | CatalogKind::PublicBrowse => ControlCommand::Browse { browse_id: query.browse_id, offset, limit: 50 },
        CatalogKind::Home | CatalogKind::Library => return Ok(CatalogPage {
            title: "Your music".into(), items: vec![], signed_in: false,
            message: if matches!(query.kind, CatalogKind::Library) { "Your library belongs to this device's account. Search public music to play on the selected device." } else { "Search for songs, albums and artists to play on the selected device." }.into(),
            continuation: String::new(), playlist_id: String::new(),
        }),
    };
    let ControlResult::Catalog {
        total,
        tracks,
        collections,
        ..
    } = request(core, command).await?
    else {
        return Err("Invalid catalog reply".into());
    };
    let mut items: Vec<_> = tracks
        .into_iter()
        .map(|t| MusicItem {
            key: format!("track:{}", t.video_id),
            title: t.title,
            subtitle: t.artist.clone(),
            artist: t.artist,
            artwork: t.artwork,
            kind: "song".into(),
            video_id: t.video_id,
            browse_id: String::new(),
        })
        .collect();
    items.extend(collections.into_iter().map(|c| MusicItem {
        key: format!("collection:{}", c.browse_id),
        title: c.title,
        subtitle: c.kind.clone(),
        artist: String::new(),
        artwork: c.artwork,
        kind: c.kind,
        video_id: String::new(),
        browse_id: c.browse_id,
    }));
    let next = offset + items.len() as u32;
    Ok(CatalogPage {
        title: "Public music".into(),
        signed_in: false,
        message: String::new(),
        continuation: if next < total && next > offset {
            format!("remote:{next}")
        } else {
            String::new()
        },
        playlist_id: String::new(),
        items,
    })
}
fn playback_command(
    status: &ClientStatus,
    command: PlayerCommand,
) -> Result<RemotePlayback, String> {
    let snapshot = status
        .snapshot
        .as_ref()
        .ok_or("Waiting for the playback device")?;
    Ok(match command {
        PlayerCommand::Play => RemotePlayback::Play,
        PlayerCommand::Pause => RemotePlayback::Pause,
        PlayerCommand::Toggle => {
            if snapshot.playing {
                RemotePlayback::Pause
            } else {
                RemotePlayback::Play
            }
        }
        PlayerCommand::Next => RemotePlayback::Next,
        PlayerCommand::Previous => RemotePlayback::Previous,
        PlayerCommand::Seek { position } => RemotePlayback::Seek { position },
        PlayerCommand::Volume { volume } => RemotePlayback::Volume { volume },
        _ => return Err("This action is available only on the playback device".into()),
    })
}
fn update(app: &AppHandle, generation: &str, f: impl FnOnce(&mut ClientStatus)) {
    let core = app.state::<Core>();
    let mut state = core.remote.lock().expect("remote lock");
    if state.generation == generation {
        f(&mut state.status);
        state.status.updated_at = crate::state::now_ms();
    }
    drop(state);
    publish(app);
}
fn publish(app: &AppHandle) {
    let status = app
        .state::<Core>()
        .remote
        .lock()
        .expect("remote lock")
        .status
        .clone();
    for label in ["settings", "mini", "party", "lyrics", "search"] {
        let _ = app.emit_to(label, "remote-state", &status);
    }
    #[cfg(target_os = "android")]
    if let Err(error) = tauri_plugin_ytm_player::remote_state(
        app,
        serde_json::to_value(&status).expect("remote state"),
    ) {
        tracing::warn!(%error, "Could not update Android remote media controls");
    }
}
async fn run(
    app: &AppHandle,
    url: &str,
    hello: ControlClientMsg,
    generation: &str,
    mut out: mpsc::Receiver<Outgoing>,
) {
    let personal = matches!(
        &hello,
        ControlClientMsg::Hello {
            mode: ControlMode::Personal,
            ..
        }
    );
    let mut remembered = false;
    let mut lost = std::time::Instant::now();
    loop {
        while let Ok((_, sender)) = out.try_recv() {
            let _ = sender.send(Err("Connection changed; command not replayed".into()));
        }
        let connection = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            tokio_tungstenite::connect_async_with_config(
                url,
                Some(
                    WebSocketConfig::default()
                        .max_message_size(Some(131072))
                        .max_frame_size(Some(131072)),
                ),
                false,
            ),
        )
        .await;
        if let Ok(Ok((mut ws, _))) = connection {
            let mut pending: HashMap<String, oneshot::Sender<Result<ControlResult, String>>> =
                HashMap::new();
            if ws
                .send(Message::Text(
                    serde_json::to_string(&hello).expect("hello").into(),
                ))
                .await
                .is_ok()
            {
                loop {
                    tokio::select! {
                        incoming = ws.next() => {
                            let raw = match incoming { Some(Ok(Message::Text(raw))) => raw, Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue, _ => break };
                            let Ok(message) = serde_json::from_str::<ControlServerMsg>(&raw) else { break; };
                            match message {
                                ControlServerMsg::Admission { participant } => update(app, generation, |s| { s.message = if participant.approved { "Connected" } else { "Waiting for host approval" }.into(); s.participant = Some(participant); }),
                                ControlServerMsg::State { snapshot } => { if personal && !remembered { remember_device(app, &snapshot.host_name, url); remembered = true; } lost = std::time::Instant::now(); update(app, generation, |s| { s.target = snapshot.host_name.clone();s.message = "Connected".into(); if let Some(p) = &s.participant { s.participant = snapshot.participants.iter().find(|peer| peer.id == p.id).cloned(); } s.snapshot = Some(snapshot); }); },
                                ControlServerMsg::Reply { id, result } => { if let Some(sender) = pending.remove(&id) { let _ = sender.send(match result { ControlResult::Error { message, .. } => Err(message), _ => Ok(result) }); } },
                                ControlServerMsg::HostAway => {update(app,generation,|s| {s.message="Host disconnected; reconnecting".into();s.snapshot=None;});for (_,sender) in pending.drain(){let _=sender.send(Err("Host disconnected; command not replayed".into()));}},
                                ControlServerMsg::Ended | ControlServerMsg::Error { .. } => {update(app,generation,|s|{s.message="Sharing ended; disconnect to choose another host".into();s.snapshot=None;});return;},
                            }
                        }
                        outgoing = out.recv() => {
                            let Some((request, sender)) = outgoing else { return; };
                            if sender.is_closed() {continue;}
                            pending.retain(|_, sender| !sender.is_closed());
                            if pending.len() >= 32 {let _=sender.send(Err("Too many pending commands".into()));continue;}
                            let raw=serde_json::to_string(&ControlClientMsg::Request {request:request.clone()}).expect("request");
                            if raw.len()>CONTROL_MAX_MESSAGE {let _=sender.send(Err("Remote request too large".into()));continue;}
                            pending.insert(request.id,sender);
                            if ws.send(Message::Text(raw.into())).await.is_err() {break;}
                        }
                    }
                }
            }
        }
        update(app, generation, |s| {
            s.message = "Connection lost; reconnecting".into();
            s.snapshot = None;
        });
        if lost.elapsed() > std::time::Duration::from_secs(600) {
            update(app, generation, |s| {
                s.message = "Session expired; disconnect and request a new invitation".into()
            });
            return;
        }
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_media_toggle_uses_the_host_and_never_falls_back_to_local_actions() {
        let snapshot = RemoteSnapshot {
            session_id: "a".repeat(64),
            host_name: "Host".into(),
            track: None,
            playing: true,
            position: 0.0,
            duration: 0.0,
            volume: 0.5,
            muted: false,
            buffering: false,
            queue_revision: 0,
            current_entry: None,
            participants: vec![],
        };
        let mut status = ClientStatus {
            active: true,
            snapshot: Some(snapshot),
            ..Default::default()
        };
        assert!(matches!(
            playback_command(&status, PlayerCommand::Toggle).unwrap(),
            RemotePlayback::Pause
        ));
        status.snapshot.as_mut().unwrap().playing = false;
        assert!(matches!(
            playback_command(&status, PlayerCommand::Toggle).unwrap(),
            RemotePlayback::Play
        ));
        assert!(playback_command(&status, PlayerCommand::Like).is_err());
        status.snapshot = None;
        assert!(playback_command(&status, PlayerCommand::Play).is_err());
    }
    #[tokio::test]
    async fn unsigned_native_search_uses_public_host_catalog_and_library_stays_local() {
        let core = Core::new(Default::default());
        let (sender, mut receiver) = mpsc::channel(4);
        let participant = ControlParticipant {
            id: "p".into(),
            name: "Phone".into(),
            mode: ControlMode::Personal,
            role: ControlRole::PersonalRemote,
            approved: true,
            connected: true,
            permissions: Default::default(),
        };
        *core.remote.lock().unwrap() = ClientState {
            status: ClientStatus {
                active: true,
                message: "Connected".into(),
                participant: Some(participant),
                ..Default::default()
            },
            out: Some(sender),
            ..Default::default()
        };
        let query = CatalogRequest {
            kind: CatalogKind::Search,
            query: "Song".into(),
            browse_id: String::new(),
            continuation: String::new(),
        };
        let work = tokio::spawn(async move {
            let (request, reply) = receiver.recv().await.unwrap();
            assert!(matches!(
                request.command,
                ControlCommand::Search {
                    offset: 0,
                    limit: 50,
                    ..
                }
            ));
            reply
                .send(Ok(ControlResult::Catalog {
                    total: 51,
                    offset: 0,
                    tracks: vec![RemoteTrack {
                        video_id: "dQw4w9WgXcQ".into(),
                        title: "Song".into(),
                        artist: "Artist".into(),
                        artwork: String::new(),
                    }],
                    collections: vec![],
                }))
                .unwrap();
            assert!(receiver.recv().await.is_none());
        });
        let page = catalog(&core, query.clone()).await.unwrap();
        assert_eq!(page.items[0].title, "Song");
        assert_eq!(page.continuation, "remote:1");
        let private = catalog(
            &core,
            CatalogRequest {
                kind: CatalogKind::Library,
                ..query
            },
        )
        .await
        .unwrap();
        assert!(private.items.is_empty());
        assert!(!private.signed_in);
        drop(core);
        work.await.unwrap();
    }
    #[test]
    fn manual_addresses_are_local_and_default_to_the_host_port() {
        assert_eq!(
            parse_address(" 192.168.1.10 ").unwrap().as_str(),
            "http://192.168.1.10:9863/"
        );
        assert_eq!(
            parse_address("192.168.1.10:9876").unwrap().port(),
            Some(9876)
        );
        for input in [
            "example.com:9863",
            "8.8.8.8:9863",
            "http://192.168.1.10/private",
            "http://192.168.1.10/?secret=x",
        ] {
            assert!(parse_address(input).is_err(), "{input}");
        }
        assert!(parse_link("192.168.1.10:9863")
            .unwrap_err()
            .contains("invitation is incomplete"));
    }
    #[tokio::test]
    async fn discovery_is_bounded_and_does_not_follow_redirects() {
        use axum::{routing::get, Router};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = Router::new().route("/session", get(|| async { "x".repeat(8193) }));
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        assert!(resolve_invitation(&address.to_string())
            .await
            .unwrap_err()
            .contains("Invalid host session response"));
        task.abort();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = Router::new().route(
            "/session",
            get(|| async { axum::response::Redirect::temporary("https://example.com/") }),
        );
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        assert!(resolve_invitation(&address.to_string())
            .await
            .unwrap_err()
            .contains("does not support address-only"));
        task.abort();
        assert!(resolve_invitation("https://relay.example/controller/")
            .await
            .unwrap_err()
            .contains("full invitation"));
    }
    #[test]
    fn invitation_rejects_insecure_internet_and_credential_injection() {
        let fragment = format!(
            "#session={}&invite={}&mode=group",
            "a".repeat(64),
            "b".repeat(64)
        );
        assert!(parse_link(&format!("http://192.168.1.2:9863/{fragment}")).is_ok());
        assert!(parse_link(&format!("https://relay.example/controller/{fragment}")).is_ok());
        assert!(parse_link(&format!("http://example.com/{fragment}")).is_err());
        assert!(parse_link(&format!("https://user:pass@example.com/{fragment}")).is_err());
        assert!(parse_link("file:///C:/secret").is_err());
    }
}
