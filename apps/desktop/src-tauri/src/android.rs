//! Android UI and session lifecycle. Desktop window conveniences have no mobile dependency.
use crate::state::Core;
use tauri::{
    ipc::{Channel, InvokeResponseBody},
    AppHandle, Manager, WebviewUrl, WebviewWindowBuilder,
};

pub fn create_main(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.clone();
    let channel = Channel::new(move |message| {
        if let InvokeResponseBody::Json(raw) = message {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) {
                let core = handle.state::<Core>();
                let result = match value["kind"].as_str() {
                    Some("google_library_account") => {
                        let payload = &value["payload"];
                        let status =
                            serde_json::from_value::<ytm_protocol::AccountStatus>(payload.clone())
                                .map_err(|e| e.to_string());
                        status.and_then(|status| {
                            crate::commands::accept_event(
                                &handle,
                                &core,
                                ytm_protocol::PlayerEvent::Account { status },
                            )?;
                            let message = payload["message"].as_str().unwrap_or("");
                            if message.len() <= 2048 {
                                core.data.write().expect("state lock").account_message =
                                    message.into();
                                core.emit(&handle);
                            }
                            Ok(())
                        })
                    }
                    Some("player_event") => serde_json::from_value(value["payload"].clone())
                        .map_err(|e| e.to_string())
                        .and_then(|event| crate::commands::accept_event(&handle, &core, event)),
                    Some("catalog_result") => serde_json::from_value(value["payload"].clone())
                        .map_err(|e| e.to_string())
                        .and_then(|reply| crate::catalog::accept_reply(&handle, &core, reply)),
                    Some("command") => serde_json::from_value(value["payload"].clone())
                        .map_err(|e| e.to_string())
                        .and_then(|command| crate::commands::user_command(&handle, &command)),
                    Some("error") => {
                        core.data.write().expect("state lock").playback_error = value["payload"]
                            .as_str()
                            .unwrap_or("Native playback error")
                            .into();
                        core.emit(&handle);
                        Ok(())
                    }
                    _ => Err("Invalid native event".into()),
                };
                if let Err(error) = result {
                    tracing::warn!(%error, "Android player event rejected");
                }
            }
        }
        Ok(())
    });
    tauri_plugin_ytm_player::start(app, include_str!("../../bridge/dist/bridge.js"), channel)
        .map_err(std::io::Error::other)?;
    Ok(())
}
pub fn show_music(app: &AppHandle) -> Result<(), String> {
    open_panel(app, "settings")
}
pub fn open_panel(app: &AppHandle, panel: &str) -> Result<(), String> {
    if !matches!(panel, "settings" | "party" | "lyrics") {
        return Err("This panel is desktop-only".into());
    }
    if app.get_webview_window("settings").is_none() {
        WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("index.html".into()))
            .title("YTM Listen")
            .build()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
pub fn open_account(app: &AppHandle) -> Result<(), String> {
    tauri_plugin_ytm_player::account(app)
}
pub fn logout_account(app: &AppHandle) -> Result<(), String> {
    tauri_plugin_ytm_player::disconnect_account(app)
}
pub fn set_app_blur(_app: &AppHandle, enabled: bool) -> Result<(), String> {
    if enabled {
        Err("Window blur is desktop-only".into())
    } else {
        Ok(())
    }
}
