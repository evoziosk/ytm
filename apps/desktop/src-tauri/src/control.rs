//! Opt-in LAN controller. Only public protocol DTOs are serialized onto this transport.
use crate::state::{Core, Snapshot};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State as HttpState,
    },
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use serde::Serialize;
use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_store::StoreExt;
use ytm_control::{secret, PairedDevice, Session};
use ytm_protocol::*;

#[derive(Default)]
pub struct ControlState {
    pub session: Option<Session>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    pub links: Vec<String>,
    pub relay_status: String,
    advertisement: Option<crate::discovery::Advertisement>,
    discovery_error: String,
}
#[derive(Serialize)]
pub struct HostStatus {
    pub session_id: String,
    pub mode: Option<ControlMode>,
    pub links: Vec<String>,
    pub participants: Vec<ControlParticipant>,
    pub paired: Vec<DeviceInfo>,
    pub relay_status: String,
    pub discovery_error: String,
}
#[derive(Serialize)]
pub struct DeviceInfo {
    id: String,
    name: String,
}
fn devices(app: &AppHandle) -> Result<Vec<PairedDevice>, String> {
    let store = app
        .store(format!("control-{}.json", crate::profile()))
        .map_err(|e| e.to_string())?;
    store
        .get("paired")
        .map(serde_json::from_value)
        .transpose()
        .map(|v| v.unwrap_or_default())
        .map_err(|e| e.to_string())
}
fn save_devices(app: &AppHandle, devices: &[PairedDevice]) -> Result<(), String> {
    let store = app
        .store(format!("control-{}.json", crate::profile()))
        .map_err(|e| e.to_string())?;
    let previous = store.get("paired");
    store.set(
        "paired",
        serde_json::to_value(devices).map_err(|e| e.to_string())?,
    );
    if let Err(error) = store.save() {
        if let Some(previous) = previous {
            store.set("paired", previous);
        } else {
            store.delete("paired");
        }
        return Err(error.to_string());
    }
    Ok(())
}
#[tauri::command]
pub async fn queue_track(
    app: AppHandle,
    core: State<'_, Core>,
    track: RemoteTrack,
    placement: QueuePlacement,
) -> Result<(), String> {
    let _guard = core.mutation.lock().await;
    if crate::control_client::active(&core) {
        let revision =
            crate::control_client::queue_revision(&core).ok_or("Waiting for host state")?;
        crate::control_client::request(
            &core,
            ControlCommand::EditQueue {
                revision,
                edit: RemoteQueueEdit::Insert {
                    tracks: vec![track],
                    placement,
                },
            },
        )
        .await?;
        return Ok(());
    }
    if core.snapshot().party.role == Some(Role::Member) {
        return Err("Leave the listening room before editing playback".into());
    }
    let command = {
        let mut data = core.data.write().expect("state lock");
        let revision = data.queue_revision;
        edit_remote(
            &mut data,
            revision,
            &RemoteQueueEdit::Insert {
                tracks: vec![track],
                placement,
            },
        )?
    };
    core.emit(&app);
    if let Some(command) = command {
        crate::playback::confirmed(&app, command).await?;
    }
    Ok(())
}
#[tauri::command]
pub fn control_status(app: AppHandle, core: State<Core>) -> Result<HostStatus, String> {
    let state = core.control.lock().expect("control lock");
    Ok(HostStatus {
        session_id: state
            .session
            .as_ref()
            .map(|s| s.id.clone())
            .unwrap_or_default(),
        mode: state.session.as_ref().map(|s| s.mode),
        links: state.links.clone(),
        participants: state
            .session
            .as_ref()
            .map(|s| s.participants.values().cloned().collect())
            .unwrap_or_default(),
        paired: devices(&app)?
            .into_iter()
            .map(|d| DeviceInfo {
                id: d.id,
                name: d.name,
            })
            .collect(),
        relay_status: state.relay_status.clone(),
        discovery_error: state.discovery_error.clone(),
    })
}

#[derive(Clone)]
struct ServerState {
    app: AppHandle,
    authorities: Arc<HashSet<String>>,
    connections: Arc<AtomicUsize>,
}
#[tauri::command]
pub async fn control_start(
    app: AppHandle,
    core: State<'_, Core>,
    mode: ControlMode,
    relay: bool,
) -> Result<(), String> {
    let _guard = core.mutation.lock().await;
    if crate::control_client::active(&core) {
        return Err("Disconnect from the remote host before sharing".into());
    }
    if core.party_stop.lock().expect("party lock").is_some() {
        return Err("Leave the listening room before sharing control".into());
    }
    if core.control.lock().expect("control lock").session.is_some() {
        return Err("End the current sharing session first".into());
    }
    let settings = core.snapshot().settings;
    if relay && settings.relay_url.is_empty() {
        return Err("Configure a relay origin in Listen together first".into());
    }
    let paired = devices(&app)?;
    let session = Session::new(settings.display_name, mode, paired);
    let address =
        if crate::profile() != "default" && std::env::args().any(|arg| arg == "--check-control") {
            "127.0.0.1:0"
        } else {
            "0.0.0.0:9863"
        };
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let mut authorities = HashSet::from([format!("127.0.0.1:{port}"), format!("localhost:{port}")]);
    for interface in if_addrs::get_if_addrs().map_err(|e| e.to_string())? {
        if let std::net::IpAddr::V4(ip) = interface.ip() {
            if !ip.is_loopback() && !ip.is_unspecified() {
                authorities.insert(format!("{ip}:{port}"));
            }
        }
    }
    let fragment = format!(
        "#session={}&invite={}&mode={}",
        session.id,
        session.invitation,
        if mode == ControlMode::Personal {
            "personal"
        } else {
            "group"
        }
    );
    let links = authorities
        .iter()
        .filter(|a| !a.starts_with("localhost"))
        .map(|a| format!("http://{a}/{fragment}"))
        .collect();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let session_id = session.id.clone();
    let announcement = if std::env::args().any(|arg| arg == "--check-control") {
        Ok(None)
    } else {
        crate::discovery::advertise(&app, &session.id, &session.host_name, mode, port).map(Some)
    };
    {
        let mut state = core.control.lock().expect("control lock");
        state.links = links;
        state.session = Some(session);
        match announcement {
            Ok(advertisement) => {
                state.advertisement = advertisement;
                state.discovery_error.clear();
            }
            Err(error) => {
                state.advertisement = None;
                state.discovery_error = format!(
                    "LAN discovery unavailable: {error}. QR and address connections still work."
                );
            }
        }
        state.stop = Some(stop);
    }
    let server = ServerState {
        app: app.clone(),
        authorities: Arc::new(authorities),
        connections: Arc::new(AtomicUsize::new(0)),
    };
    let router = Router::new()
        .route("/", get(index))
        .route("/controller.js", get(script))
        .route("/controller.css", get(style))
        .route("/session", get(discover))
        .route("/ws", get(upgrade))
        .with_state(server);
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
        {
            tracing::warn!(%error, "LAN controller stopped");
            let core = handle.state::<Core>();
            let mut state = core.control.lock().expect("control lock");
            if state.session.as_ref().is_some_and(|s| s.id == session_id) {
                state.session = None;
                state.advertisement = None;
                state.links.clear();
            }
        }
    });
    if relay {
        super::control_relay::start(&app, &core, &settings.relay_url);
    }
    Ok(())
}

#[tauri::command]
pub async fn control_end(core: State<'_, Core>) -> Result<(), String> {
    let _guard = core.mutation.lock().await;
    let mut state = core.control.lock().expect("control lock");
    state.session = None;
    state.advertisement = None;
    state.discovery_error.clear();
    state.links.clear();
    state.relay_status.clear();
    if let Some(stop) = state.stop.take() {
        let _ = stop.send(());
    }
    Ok(())
}
#[tauri::command]
pub async fn control_participant(
    app: AppHandle,
    core: State<'_, Core>,
    id: String,
    permissions: Option<ControlPermissions>,
) -> Result<(), String> {
    let _guard = core.mutation.lock().await;
    let mut state = core.control.lock().expect("control lock");
    if let Some(session) = state.session.as_mut() {
        let mut candidate = session.clone();
        if let Some(permissions) = permissions {
            candidate.approve(&id, permissions)?;
        } else {
            candidate.remove(&id);
        }
        save_devices(&app, &candidate.paired)?;
        *session = candidate;
    } else if permissions.is_none() {
        let mut paired = devices(&app)?;
        paired.retain(|d| d.id != id);
        save_devices(&app, &paired)?;
    } else {
        return Err("Sharing is disabled".into());
    }
    Ok(())
}
#[tauri::command]
pub async fn control_rotate(core: State<'_, Core>) -> Result<(), String> {
    let _guard = core.mutation.lock().await;
    let mut state = core.control.lock().expect("control lock");
    let session = state.session.as_mut().ok_or("Sharing is disabled")?;
    let old = session.invitation.clone();
    session.rotate();
    let new = session.invitation.clone();
    for link in &mut state.links {
        *link = link.replace(&old, &new);
    }
    Ok(())
}

fn valid_headers(headers: &HeaderMap, server: &ServerState) -> bool {
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !server.authorities.contains(host) {
        return false;
    }
    // Browsers must use the controller served by this exact host, preventing DNS rebinding/CSWSH.
    headers
        .get(header::ORIGIN)
        .map(|v| {
            v.to_str()
                .is_ok_and(|origin| origin == format!("http://{host}"))
        })
        .unwrap_or(true)
}
fn asset(
    headers: HeaderMap,
    server: ServerState,
    body: &'static str,
    content_type: &'static str,
) -> Response {
    if !valid_headers(&headers, &server) {
        return StatusCode::FORBIDDEN.into_response();
    }
    ([ (header::CONTENT_TYPE, content_type), (header::CACHE_CONTROL, "no-store"), (header::REFERRER_POLICY, "no-referrer"), (header::X_CONTENT_TYPE_OPTIONS, "nosniff"), (header::CONTENT_SECURITY_POLICY, "default-src 'self'; script-src 'self'; style-src 'self'; img-src https:; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'") ], body).into_response()
}
async fn index(HttpState(s): HttpState<ServerState>, h: HeaderMap) -> Response {
    asset(
        h,
        s,
        include_str!("../../../controller/index.html"),
        "text/html;charset=utf-8",
    )
}
async fn script(HttpState(s): HttpState<ServerState>, h: HeaderMap) -> Response {
    asset(
        h,
        s,
        include_str!("../../../controller/controller.js"),
        "text/javascript;charset=utf-8",
    )
}
async fn style(HttpState(s): HttpState<ServerState>, h: HeaderMap) -> Response {
    asset(
        h,
        s,
        include_str!("../../../controller/controller.css"),
        "text/css;charset=utf-8",
    )
}
async fn discover(HttpState(server): HttpState<ServerState>, headers: HeaderMap) -> Response {
    if !valid_headers(&headers, &server) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let core = server.app.state::<Core>();
    let state = core.control.lock().expect("control lock");
    let Some(session) = state.session.as_ref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let public = ControlDiscovery {
        version: CONTROL_VERSION,
        session_id: session.id.clone(),
        host_name: session.host_name.clone(),
        mode: session.mode,
    };
    (
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        axum::Json(public),
    )
        .into_response()
}
async fn upgrade(
    HttpState(s): HttpState<ServerState>,
    h: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !valid_headers(&h, &s) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if s.connections.fetch_add(1, Ordering::SeqCst) >= CONTROL_MAX_CONTROLLERS {
        s.connections.fetch_sub(1, Ordering::SeqCst);
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    ws.max_message_size(CONTROL_MAX_MESSAGE)
        .max_frame_size(CONTROL_MAX_MESSAGE)
        .on_upgrade(move |socket| async move {
            serve_socket(socket, &s.app).await;
            s.connections.fetch_sub(1, Ordering::SeqCst);
        })
}

pub(crate) fn hello(core: &Core, raw: &str) -> Result<ControlParticipant, String> {
    accept_hello(core, raw, false)
}
fn accept_hello(core: &Core, raw: &str, local_address: bool) -> Result<ControlParticipant, String> {
    let ControlClientMsg::Hello {
        version,
        session_id,
        invitation,
        credential,
        name,
        mode,
    } = serde_json::from_str(raw).map_err(|_| "Invalid hello")?
    else {
        return Err("Say hello first".into());
    };
    let mut state = core.control.lock().expect("control lock");
    let session = state.session.as_mut().ok_or("Sharing is disabled")?;
    // An address-only request can reach pending admission over LAN. It never grants
    // access by itself. Relayed connections still require the invitation secret.
    let invitation = if local_address && invitation.is_empty() {
        session.invitation.clone()
    } else {
        invitation
    };
    session.connect(version, &session_id, &invitation, &credential, &name, mode)
}
pub(crate) fn disconnect(core: &Core, session_id: &str, peer: &str) {
    if let Some(s) = core
        .control
        .lock()
        .expect("control lock")
        .session
        .as_mut()
        .filter(|s| s.id == session_id)
    {
        s.disconnect(peer);
    }
}
async fn send(ws: &mut WebSocket, message: &ControlServerMsg) -> bool {
    ws.send(Message::Text(
        serde_json::to_string(message)
            .expect("public message")
            .into(),
    ))
    .await
    .is_ok()
}
async fn serve_socket(mut ws: WebSocket, app: &AppHandle) {
    let core = app.state::<Core>();
    let first = tokio::time::timeout(std::time::Duration::from_secs(10), ws.recv()).await;
    let Ok(Some(Ok(Message::Text(raw)))) = first else {
        return;
    };
    let peer = match accept_hello(&core, &raw, true) {
        Ok(p) => p,
        Err(message) => {
            send(&mut ws, &ControlServerMsg::Error { message }).await;
            return;
        }
    };
    let session_id = core
        .control
        .lock()
        .expect("control lock")
        .session
        .as_ref()
        .map(|s| s.id.clone())
        .unwrap_or_default();
    send(
        &mut ws,
        &ControlServerMsg::Admission {
            participant: peer.clone(),
        },
    )
    .await;
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
    let mut rate_window = std::time::Instant::now();
    let mut count = 0;
    loop {
        tokio::select! {
            _ = tick.tick() => {
                let message = peer_state(&core, &session_id, &peer.id);
                let ended = matches!(message, ControlServerMsg::Ended);
                if !send(&mut ws, &message).await || ended { break; }
            }
            message = ws.recv() => {
                let raw = match message { Some(Ok(Message::Text(raw))) => raw, Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue, _ => break };
                if rate_window.elapsed().as_secs() >= 10 { count = 0; rate_window = std::time::Instant::now(); }
                count += 1; if count > 40 { break; }
                let Ok(ControlClientMsg::Request { request }) = serde_json::from_str(&raw) else { break; };
                let result = execute(app, &session_id, &peer.id, &request).await;
                if !send(&mut ws, &ControlServerMsg::Reply { id: request.id, result }).await { break; }
            }
        }
    }
    disconnect(&core, &session_id, &peer.id);
}
pub(crate) fn peer_state(core: &Core, session_id: &str, peer: &str) -> ControlServerMsg {
    let state = core.control.lock().expect("control lock");
    let Some(s) = state.session.as_ref().filter(|s| s.id == session_id) else {
        return ControlServerMsg::Ended;
    };
    match s.participants.get(peer) {
        Some(p) if p.approved => ControlServerMsg::State {
            snapshot: public_snapshot(&core.data.read().expect("state lock"), s),
        },
        Some(p) => ControlServerMsg::Admission {
            participant: p.clone(),
        },
        None => ControlServerMsg::Ended,
    }
}
pub fn public_snapshot(data: &Snapshot, session: &Session) -> RemoteSnapshot {
    RemoteSnapshot {
        session_id: session.id.clone(),
        host_name: session.host_name.clone(),
        track: data.player.track.as_ref().map(|t| RemoteTrack {
            video_id: t.video_id.clone(),
            title: t.title.clone(),
            artist: t.artist.clone(),
            artwork: t.artwork.clone(),
        }),
        playing: data.player.playing,
        position: data.player.position,
        duration: data.player.duration,
        volume: data.player.volume,
        muted: data.player.muted,
        buffering: data.player.buffering,
        queue_revision: data.queue_revision,
        current_entry: data.queue.get(data.queue_index).map(|i| i.key.clone()),
        participants: session
            .participants
            .values()
            .filter(|p| p.approved)
            .cloned()
            .collect(),
    }
}
pub(crate) fn remote_track(item: &MusicItem) -> RemoteTrack {
    RemoteTrack {
        video_id: item.video_id.clone(),
        title: item.title.clone(),
        artist: item.artist.clone(),
        artwork: item.artwork.clone(),
    }
}
fn error(code: &str, message: impl Into<String>) -> ControlResult {
    ControlResult::Error {
        code: code.into(),
        message: message.into(),
    }
}
pub(crate) async fn execute(
    app: &AppHandle,
    session_id: &str,
    peer: &str,
    request: &ControlRequest,
) -> ControlResult {
    let core = app.state::<Core>();
    let _guard = core.mutation.lock().await;
    {
        let state = core.control.lock().expect("control lock");
        let Some(s) = state.session.as_ref().filter(|s| s.id == session_id) else {
            return error("ended", "Sharing ended");
        };
        if let Err(message) = s.authorize(peer, &request.command) {
            return error("forbidden", message);
        }
        match s.previous(peer, request) {
            Ok(Some(result)) => return result,
            Err(message) => return error("request_id", message),
            _ => (),
        }
    }
    let result = execute_command(app, &core, &request.command)
        .await
        .unwrap_or_else(|e| error("command_failed", e));
    if let Some(s) = core
        .control
        .lock()
        .expect("control lock")
        .session
        .as_mut()
        .filter(|s| s.id == session_id)
    {
        s.remember(peer, request, result.clone());
    }
    result
}
async fn execute_command(
    app: &AppHandle,
    core: &Core,
    command: &ControlCommand,
) -> Result<ControlResult, String> {
    match command {
        ControlCommand::Snapshot => {
            let state = core.control.lock().expect("control lock");
            Ok(ControlResult::Snapshot {
                snapshot: public_snapshot(
                    &core.data.read().expect("state lock"),
                    state.session.as_ref().ok_or("Sharing ended")?,
                ),
            })
        }
        ControlCommand::Queue { offset, limit } => {
            let (offset, limit) = page(*offset, *limit)?;
            let data = core.data.read().expect("state lock");
            let mut bytes = 0;
            let entries = data
                .queue
                .iter()
                .skip(offset)
                .take(limit)
                .map(|i| RemoteQueueEntry {
                    id: i.key.clone(),
                    track: remote_track(i),
                })
                .take_while(|entry| {
                    bytes += serde_json::to_vec(entry).expect("queue entry").len();
                    bytes <= 48000
                })
                .collect();
            Ok(ControlResult::Queue {
                revision: data.queue_revision,
                total: data.queue.len() as u32,
                offset: offset as u32,
                entries,
            })
        }
        ControlCommand::Playback { command } => {
            let command = match command {
                RemotePlayback::Play => PlayerCommand::Play,
                RemotePlayback::Pause => PlayerCommand::Pause,
                RemotePlayback::Next => PlayerCommand::Next,
                RemotePlayback::Previous => PlayerCommand::Previous,
                RemotePlayback::Seek { position } => PlayerCommand::Seek {
                    position: *position,
                },
                RemotePlayback::Volume { volume } => PlayerCommand::Volume { volume: *volume },
            };
            crate::commands::perform_serialized(app, command).await?;
            Ok(ControlResult::Ok)
        }
        ControlCommand::EditQueue { revision, edit } => {
            let command =
                edit_remote(&mut core.data.write().expect("state lock"), *revision, edit)?;
            core.emit(app);
            if let Some(command) = command {
                crate::playback::confirmed(app, command).await?;
            }
            Ok(ControlResult::Ok)
        }
        ControlCommand::Search {
            query,
            offset,
            limit,
        } => {
            let (offset, limit) = page(*offset, *limit)?;
            if query.trim().is_empty() || query.len() > 512 {
                return Err("Search must have 1–512 bytes".into());
            }
            let page = crate::catalog::query(
                app,
                core,
                CatalogRequest {
                    kind: CatalogKind::PublicSearch,
                    query: query.clone(),
                    browse_id: String::new(),
                    continuation: String::new(),
                },
            )
            .await?;
            Ok(catalog_page(page, offset, limit))
        }
        ControlCommand::Browse {
            browse_id,
            offset,
            limit,
        } => {
            let (offset, limit) = page(*offset, *limit)?;
            if !public_browse(browse_id) {
                return Err("Only public collections can be browsed".into());
            }
            let page = crate::catalog::query(
                app,
                core,
                CatalogRequest {
                    kind: CatalogKind::PublicBrowse,
                    query: String::new(),
                    browse_id: browse_id.clone(),
                    continuation: String::new(),
                },
            )
            .await?;
            Ok(catalog_page(page, offset, limit))
        }
    }
}
fn public_browse(id: &str) -> bool {
    id.len() <= 256
        && ["UC", "MPRE", "VL"]
            .iter()
            .any(|prefix| id.starts_with(prefix) && id.len() > prefix.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn catalog_page(page: CatalogPage, offset: usize, limit: usize) -> ControlResult {
    let items: Vec<_> = page
        .items
        .into_iter()
        .filter(|i| valid_video_id(&i.video_id) || public_browse(&i.browse_id))
        .collect();
    let total = items.len() as u32;
    let mut tracks = Vec::new();
    let mut collections = Vec::new();
    let mut bytes = 0;
    for item in items.into_iter().skip(offset).take(limit) {
        bytes += serde_json::to_vec(&item).expect("catalog item").len();
        if bytes > 48000 {
            break;
        }
        if valid_video_id(&item.video_id) {
            tracks.push(remote_track(&item));
        } else {
            collections.push(RemoteCollection {
                browse_id: item.browse_id,
                title: item.title,
                kind: item.kind,
                artwork: item.artwork,
            });
        }
    }
    ControlResult::Catalog {
        total,
        offset: offset as u32,
        tracks,
        collections,
    }
}
fn page(offset: u32, limit: u32) -> Result<(usize, usize), String> {
    if limit == 0 || limit as usize > CONTROL_PAGE_SIZE || offset > 5000 {
        return Err("Invalid page (maximum 50 items)".into());
    }
    Ok((offset as usize, limit as usize))
}
pub fn edit_remote(
    data: &mut Snapshot,
    revision: u32,
    edit: &RemoteQueueEdit,
) -> Result<Option<PlayerCommand>, String> {
    if revision != data.queue_revision {
        return Err("Queue changed; refresh before editing".into());
    }
    let mut load = None;
    match edit {
        RemoteQueueEdit::Select { entry_id } => {
            let index = data
                .queue
                .iter()
                .position(|item| &item.key == entry_id)
                .ok_or("Entry no longer exists")?;
            data.queue_index = index;
            load = Some(PlayerCommand::LoadTrack {
                video_id: data.queue[index].video_id.clone(),
            });
        }
        RemoteQueueEdit::Insert { tracks, placement } => {
            if tracks.is_empty()
                || tracks.len() > 50
                || data.queue.len() + tracks.len() > 5000
                || tracks.iter().any(|t| !ytm_control::validate_track(t))
            {
                return Err("Invalid tracks or queue limit reached".into());
            }
            let items: Vec<_> = tracks
                .iter()
                .map(|t| MusicItem {
                    key: secret(),
                    title: t.title.clone(),
                    artist: t.artist.clone(),
                    subtitle: t.artist.clone(),
                    artwork: t.artwork.clone(),
                    kind: "song".into(),
                    video_id: t.video_id.clone(),
                    browse_id: String::new(),
                })
                .collect();
            let insert = if matches!(placement, QueuePlacement::Replace) {
                data.queue.clear();
                data.queue_playlist_id.clear();
                data.previous_playlist = None;
                data.queue_index = 0;
                0
            } else if matches!(placement, QueuePlacement::End) {
                data.queue.len()
            } else {
                (data.queue_index + 1).min(data.queue.len())
            };
            let play = matches!(placement, QueuePlacement::Now | QueuePlacement::Replace)
                || data.queue.is_empty();
            data.queue.splice(insert..insert, items);
            if play {
                data.queue_index = insert;
                load = Some(PlayerCommand::LoadTrack {
                    video_id: data.queue[insert].video_id.clone(),
                });
            }
        }
        RemoteQueueEdit::Remove { entry_id } => {
            let index = data
                .queue
                .iter()
                .position(|i| &i.key == entry_id)
                .ok_or("Entry no longer exists")?;
            crate::queue::edit(data, &crate::queue::QueueAction::Remove { index })?;
            return Ok(None);
        }
        RemoteQueueEdit::Move {
            entry_id,
            before_id,
        } => {
            let from = data
                .queue
                .iter()
                .position(|i| &i.key == entry_id)
                .ok_or("Entry no longer exists")?;
            let before = if before_id.is_empty() {
                data.queue.len()
            } else {
                data.queue
                    .iter()
                    .position(|i| &i.key == before_id)
                    .ok_or("Target no longer exists")?
            };
            let to = if from < before {
                before.saturating_sub(1)
            } else {
                before
            };
            crate::queue::edit(data, &crate::queue::QueueAction::Move { from, to })?;
            return Ok(None);
        }
        RemoteQueueEdit::Clear => {
            crate::queue::edit(data, &crate::queue::QueueAction::Clear)?;
            return Ok(None);
        }
    }
    data.queue_revision = data.queue_revision.wrapping_add(1);
    Ok(load)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn track() -> RemoteTrack {
        RemoteTrack {
            video_id: "dQw4w9WgXcQ".into(),
            title: "Song".into(),
            artist: "Artist".into(),
            artwork: String::new(),
        }
    }
    #[test]
    fn queue_revisions_duplicate_tracks_and_play_now_preserve_remaining() {
        let mut data = Core::new(Default::default()).snapshot();
        edit_remote(
            &mut data,
            0,
            &RemoteQueueEdit::Insert {
                tracks: vec![track(), track()],
                placement: QueuePlacement::End,
            },
        )
        .unwrap();
        assert_ne!(data.queue[0].key, data.queue[1].key);
        let remaining = data.queue[1].key.clone();
        assert!(edit_remote(&mut data, 0, &RemoteQueueEdit::Clear).is_err());
        edit_remote(
            &mut data,
            1,
            &RemoteQueueEdit::Insert {
                tracks: vec![track()],
                placement: QueuePlacement::Now,
            },
        )
        .unwrap();
        assert_eq!(data.queue.len(), 3);
        assert_eq!(data.queue[2].key, remaining);
        assert_eq!(data.queue_index, 1);
        let revision = data.queue_revision;
        let ids: Vec<_> = data.queue.iter().map(|item| item.key.clone()).collect();
        let selected = edit_remote(
            &mut data,
            revision,
            &RemoteQueueEdit::Select {
                entry_id: remaining.clone(),
            },
        )
        .unwrap();
        assert!(matches!(selected, Some(PlayerCommand::LoadTrack { .. })));
        assert_eq!(data.queue[data.queue_index].key, remaining);
        assert_eq!(
            data.queue
                .iter()
                .map(|item| item.key.clone())
                .collect::<Vec<_>>(),
            ids
        );
        let revision = data.queue_revision;
        edit_remote(
            &mut data,
            revision,
            &RemoteQueueEdit::Move {
                entry_id: ids[0].clone(),
                before_id: String::new(),
            },
        )
        .unwrap();
        assert_eq!(data.queue.last().unwrap().key, ids[0]);
    }
    #[test]
    fn snapshot_has_no_account_settings_or_collection_identifiers() {
        let mut data = Core::new(Default::default()).snapshot();
        data.settings.discord_app_id = "SECRET".into();
        data.account.name = "SECRET".into();
        data.queue_playlist_id = "SECRET".into();
        data.player.track = Some(TrackInfo {
            playlist_id: "SECRET".into(),
            ..Default::default()
        });
        let s = Session::new("Host".into(), ControlMode::Group, vec![]);
        let json = serde_json::to_string(&public_snapshot(&data, &s)).unwrap();
        assert!(!json.contains("SECRET"));
        assert!(!json.contains("settings"));
        assert!(!json.contains("account"));
        assert!(!json.contains(&s.invitation));
    }
}
