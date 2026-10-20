//! Outbound relay transport. Admission and commands still execute on the live host.
use crate::{control, state::Core};
use futures_util::{SinkExt, StreamExt};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};
use tokio_tungstenite::tungstenite::{protocol::WebSocketConfig, Message};
use ytm_protocol::*;

pub fn start(app: &AppHandle, core: &Core, origin: &str) {
    let (id, token) = {
        let mut state = core.control.lock().expect("control lock");
        let Some(s) = state.session.as_ref() else {
            return;
        };
        let id = s.id.clone();
        let link = format!(
            "{origin}/controller/#session={}&invite={}&mode={}",
            s.id,
            s.invitation,
            if s.mode == ControlMode::Personal {
                "personal"
            } else {
                "group"
            }
        );
        state.links.push(link);
        state.relay_status = "Connecting".into();
        (id, ytm_control::secret())
    };
    let app = app.clone();
    let origin = origin.to_string();
    tauri::async_runtime::spawn(async move {
        let core = app.state::<Core>();
        let mut url = match url::Url::parse(&origin) {
            Ok(u) => u,
            Err(_) => return,
        };
        let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
        if url.set_scheme(scheme).is_err() {
            return;
        }
        url.set_path(&format!("/control/{id}"));
        let mut disconnected = Instant::now();
        loop {
            let mut authenticated = false;
            if !current(&core, &id) {
                return;
            }
            status(&core, &id, "Connecting");
            let result = tokio::time::timeout(
                Duration::from_secs(15),
                tokio_tungstenite::connect_async_with_config(
                    url.as_str(),
                    Some(
                        WebSocketConfig::default()
                            .max_message_size(Some(131072))
                            .max_frame_size(Some(131072)),
                    ),
                    false,
                ),
            )
            .await;
            if let Ok(Ok((mut ws, _))) = result {
                let hello = serde_json::json!({"type":"host", "token":token});
                if ws
                    .send(Message::Text(hello.to_string().into()))
                    .await
                    .is_ok()
                {
                    let mut peers: HashMap<String, String> = HashMap::new();
                    let mut tick = tokio::time::interval(Duration::from_secs(1));
                    loop {
                        tokio::select! {
                            _ = tick.tick() => {
                                if !current(&core, &id) { let _ = ws.send(Message::Text(serde_json::json!({"type":"end"}).to_string().into())).await; let _ = ws.close(None).await; break; }
                                for (connection, peer) in &peers {
                                    let message = control::peer_state(&core, &id, peer);
                                    let payload = serde_json::json!({"type":"deliver", "connection_id":connection, "message":message});
                                    if ws.send(Message::Text(payload.to_string().into())).await.is_err() { break; }
                                }
                            }
                            incoming = ws.next() => {
                                let raw = match incoming { Some(Ok(Message::Text(raw))) => raw, Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue, _ => break };
                                let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else { break; };
                                if value["type"] == "ready" { authenticated = true; status(&core, &id, "Connected"); continue; }
                                if value["type"] == "expired" { status(&core, &id, "Expired; end sharing and create a new session"); return; }
                                let Some(connection) = value["connection_id"].as_str() else { continue; };
                                if value["type"] == "disconnected" {
                                    if let Some(peer) = peers.remove(connection) { control::disconnect(&core, &id, &peer); } continue;
                                }
                                if value["type"] != "controller" { continue; }
                                let raw = value["message"].to_string();
                                let message = if let Some(peer) = peers.get(connection) {
                                    match serde_json::from_str::<ControlClientMsg>(&raw) {
                                        Ok(ControlClientMsg::Request { request }) => ControlServerMsg::Reply { id: request.id.clone(), result: control::execute(&app, &id, peer, &request).await },
                                        _ => ControlServerMsg::Error { message: "Invalid command".into() },
                                    }
                                } else {
                                    match control::hello(&core, &raw) {
                                        Ok(peer) => { peers.insert(connection.into(), peer.id.clone()); ControlServerMsg::Admission { participant: peer } },
                                        Err(message) => ControlServerMsg::Error { message },
                                    }
                                };
                                let payload = serde_json::json!({"type":"deliver", "connection_id":connection, "message":message});
                                if ws.send(Message::Text(payload.to_string().into())).await.is_err() { break; }
                            }
                        }
                    }
                    for peer in peers.values() {
                        control::disconnect(&core, &id, peer);
                    }
                }
            }
            if authenticated {
                disconnected = Instant::now();
            }
            if disconnected.elapsed() > Duration::from_secs(600) {
                status(&core, &id, "Expired; end sharing and create a new session");
                return;
            }
            status(&core, &id, "Host connection lost; reconnecting");
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    });
}
fn current(core: &Core, id: &str) -> bool {
    core.control
        .lock()
        .expect("control lock")
        .session
        .as_ref()
        .is_some_and(|s| s.id == id)
}
fn status(core: &Core, id: &str, message: &str) {
    let mut state = core.control.lock().expect("control lock");
    if state.session.as_ref().is_some_and(|s| s.id == id) {
        state.relay_status = message.into();
    }
}
