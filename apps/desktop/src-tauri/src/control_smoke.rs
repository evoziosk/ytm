//! Explicit opt-in network smoke test, using an isolated --profile and no audible playback.
use crate::{control, state::Core};
use futures_util::{SinkExt, StreamExt};
use tauri::{AppHandle, Manager};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
    MaybeTlsStream, WebSocketStream,
};
use ytm_protocol::*;
type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
async fn next(ws: &mut Socket) -> Result<ControlServerMsg, String> {
    loop {
        let message = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next())
            .await
            .map_err(|_| "Smoke reply timeout")?
            .ok_or("Smoke socket closed")?
            .map_err(|e| e.to_string())?;
        if let Message::Text(raw) = message {
            return serde_json::from_str(&raw).map_err(|e| e.to_string());
        }
    }
}
async fn reply(ws: &mut Socket, request: &ControlRequest) -> Result<ControlResult, String> {
    ws.send(Message::Text(
        serde_json::to_string(&ControlClientMsg::Request {
            request: request.clone(),
        })
        .expect("request")
        .into(),
    ))
    .await
    .map_err(|e| e.to_string())?;
    loop {
        if let ControlServerMsg::Reply { id, result } = next(ws).await? {
            if id == request.id {
                return Ok(result);
            }
        }
    }
}
pub async fn run(app: &AppHandle) -> Result<(), String> {
    if crate::profile() == "default" {
        return Err("Use --profile control-smoke for an isolated check".into());
    }
    let core = app.state::<Core>();
    control::control_start(app.clone(), app.state::<Core>(), ControlMode::Group, false).await?;
    let result = exercise(app).await;
    control::control_end(app.state::<Core>()).await?;
    if core.control.lock().expect("control lock").session.is_some() {
        return Err("Session did not end".into());
    }
    result
}
async fn exercise(app: &AppHandle) -> Result<(), String> {
    let core = app.state::<Core>();
    let status = control::control_status(app.clone(), app.state::<Core>())?;
    let link = status
        .links
        .iter()
        .find(|link| link.starts_with("http://127.0.0.1:"))
        .ok_or("Missing smoke address")?;
    let parsed = url::Url::parse(link).map_err(|e| e.to_string())?;
    let address = format!("127.0.0.1:{}", parsed.port().ok_or("Missing smoke port")?);
    let origin = format!("http://{address}");
    let socket_url = format!("ws://{address}/ws");
    let http = reqwest::Client::new();
    let page = http.get(&origin).send().await.map_err(|e| e.to_string())?;
    if page.status() != 200 || !page.headers().contains_key("content-security-policy") {
        return Err("Controller asset unavailable".into());
    }
    let (url, session_id, invitation, mode) =
        crate::control_client::resolve_invitation(&address).await?;
    let metadata = http
        .get(format!("{origin}/session"))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    if !invitation.is_empty()
        || !metadata.contains(&session_id)
        || metadata.contains("invitation")
        || metadata.contains("settings")
        || url.port() != parsed.port()
    {
        return Err("LAN address resolution leaked data or returned an invalid session".into());
    }
    let bad = http
        .get(&origin)
        .header("Host", "rebind.invalid:9863")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if bad.status() != 403 {
        return Err("Host validation failed".into());
    }
    let mut request = socket_url
        .as_str()
        .into_client_request()
        .map_err(|e| e.to_string())?;
    request.headers_mut().insert(
        "Origin",
        "https://attacker.invalid".parse().expect("header"),
    );
    if connect_async(request).await.is_ok() {
        return Err("Origin validation failed".into());
    }
    let hello = ControlClientMsg::Hello {
        version: 1,
        session_id,
        invitation,
        credential: ytm_control::secret(),
        name: "Smoke controller".into(),
        mode,
    };
    if control::hello(&core, &serde_json::to_string(&hello).expect("hello")).is_ok() {
        return Err("Relay admitted a guest without an invitation".into());
    }
    let (mut ws, _) = connect_async(&socket_url)
        .await
        .map_err(|e| e.to_string())?;
    ws.send(Message::Text(
        serde_json::to_string(&hello).expect("hello").into(),
    ))
    .await
    .map_err(|e| e.to_string())?;
    let ControlServerMsg::Admission { participant } = next(&mut ws).await? else {
        return Err("Missing admission".into());
    };
    if participant.approved {
        return Err("Guest admitted without approval".into());
    }
    let request = ControlRequest {
        id: "a".repeat(32),
        command: ControlCommand::Snapshot,
    };
    if !matches!(reply(&mut ws, &request).await?, ControlResult::Error { .. }) {
        return Err("Unapproved state access".into());
    }
    control::control_participant(
        app.clone(),
        app.state::<Core>(),
        participant.id.clone(),
        Some(Default::default()),
    )
    .await?;
    let state = reply(&mut ws, &request).await?;
    if !matches!(state, ControlResult::Snapshot { .. }) {
        return Err("Approved snapshot unavailable".into());
    }
    let volume = ControlRequest {
        id: "b".repeat(32),
        command: ControlCommand::Playback {
            command: RemotePlayback::Volume { volume: 0.5 },
        },
    };
    if !matches!(reply(&mut ws, &volume).await?, ControlResult::Error { .. }) {
        return Err("Guest volume permission bypass".into());
    }
    let track = RemoteTrack {
        video_id: "dQw4w9WgXcQ".into(),
        title: "Smoke fixture".into(),
        artist: String::new(),
        artwork: String::new(),
    };
    let (revision, entry) = {
        let mut data = core.data.write().expect("state lock");
        let revision = data.queue_revision;
        control::edit_remote(
            &mut data,
            revision,
            &RemoteQueueEdit::Insert {
                tracks: vec![track.clone(), track],
                placement: QueuePlacement::End,
            },
        )?;
        (data.queue_revision, data.queue[1].key.clone())
    };
    let edit = ControlRequest {
        id: "c".repeat(32),
        command: ControlCommand::EditQueue {
            revision,
            edit: RemoteQueueEdit::Remove { entry_id: entry },
        },
    };
    if !matches!(reply(&mut ws, &edit).await?, ControlResult::Ok) {
        return Err("Queue removal failed".into());
    }
    if !matches!(reply(&mut ws, &edit).await?, ControlResult::Ok)
        || core.snapshot().queue.len() != 1
    {
        return Err("Duplicate command reapplied".into());
    }
    let stale = ControlRequest {
        id: "d".repeat(32),
        command: edit.command.clone(),
    };
    if !matches!(reply(&mut ws, &stale).await?, ControlResult::Error { .. }) {
        return Err("Stale queue revision accepted".into());
    }
    let (revision, first_entry, second_entry) = {
        let mut data = core.data.write().expect("state lock");
        let track = control::remote_track(&data.queue[0]);
        let revision = data.queue_revision;
        control::edit_remote(
            &mut data,
            revision,
            &RemoteQueueEdit::Insert {
                tracks: vec![track.clone(), track],
                placement: QueuePlacement::End,
            },
        )?;
        (
            data.queue_revision,
            data.queue[1].key.clone(),
            data.queue[2].key.clone(),
        )
    };
    let mut other_hello = hello.clone();
    if let ControlClientMsg::Hello {
        credential, name, ..
    } = &mut other_hello
    {
        *credential = ytm_control::secret();
        *name = "Concurrent controller".into();
    }
    let (mut other, _) = connect_async(&socket_url)
        .await
        .map_err(|e| e.to_string())?;
    other
        .send(Message::Text(
            serde_json::to_string(&other_hello).expect("hello").into(),
        ))
        .await
        .map_err(|e| e.to_string())?;
    let ControlServerMsg::Admission {
        participant: second,
    } = next(&mut other).await?
    else {
        return Err("Missing second admission".into());
    };
    control::control_participant(
        app.clone(),
        app.state::<Core>(),
        second.id,
        Some(Default::default()),
    )
    .await?;
    let first = ControlRequest {
        id: "e".repeat(32),
        command: ControlCommand::EditQueue {
            revision,
            edit: RemoteQueueEdit::Remove {
                entry_id: first_entry,
            },
        },
    };
    let second = ControlRequest {
        id: "f".repeat(32),
        command: ControlCommand::EditQueue {
            revision,
            edit: RemoteQueueEdit::Remove {
                entry_id: second_entry,
            },
        },
    };
    let (first, second) = tokio::join!(reply(&mut ws, &first), reply(&mut other, &second));
    let results = [first?, second?];
    if results
        .iter()
        .filter(|r| matches!(r, ControlResult::Ok))
        .count()
        != 1
        || core.snapshot().queue.len() != 2
    {
        return Err("Concurrent stale edit accepted".into());
    }
    other.close(None).await.map_err(|e| e.to_string())?;
    ws.close(None).await.map_err(|e| e.to_string())?;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let (mut ws, _) = connect_async(&socket_url)
        .await
        .map_err(|e| e.to_string())?;
    ws.send(Message::Text(
        serde_json::to_string(&hello).expect("hello").into(),
    ))
    .await
    .map_err(|e| e.to_string())?;
    if !matches!(
        next(&mut ws).await?,
        ControlServerMsg::Admission {
            participant: ControlParticipant { approved: true, .. }
        }
    ) {
        return Err("Session reconnect lost admission".into());
    }
    if !matches!(reply(&mut ws, &edit).await?, ControlResult::Ok)
        || core.snapshot().queue.len() != 2
    {
        return Err("Reconnect replay applied twice".into());
    }
    control::control_participant(app.clone(), app.state::<Core>(), participant.id, None).await?;
    loop {
        if matches!(next(&mut ws).await?, ControlServerMsg::Ended) {
            break;
        }
    }
    Ok(())
}
