//! Native Android session adapter. No Google credentials leave the service WebView.
#[cfg(target_os = "android")]
use tauri::{
    plugin::{Builder, PluginHandle, TauriPlugin},
    AppHandle, Manager, Wry,
};
#[cfg(target_os = "android")]
struct Player(PluginHandle<Wry>);

#[cfg(target_os = "android")]
pub fn init() -> TauriPlugin<Wry> {
    Builder::new("ytm-player")
        .setup(|app, api| {
            let handle = api.register_android_plugin("io.ytmlisten.player", "PlayerPlugin")?;
            app.manage(Player(handle));
            Ok(())
        })
        .build()
}
#[cfg(target_os = "android")]
pub fn start(app: &AppHandle, bridge: &str, handler: tauri::ipc::Channel) -> Result<(), String> {
    #[derive(serde::Serialize)]
    struct Args<'a> {
        bridge: &'a str,
        handler: tauri::ipc::Channel,
    }
    app.state::<Player>()
        .0
        .run_mobile_plugin::<()>("initialize", Args { bridge, handler })
        .map_err(|e| e.to_string())
}
#[cfg(target_os = "android")]
pub fn evaluate(app: &AppHandle, script: &str) -> Result<(), String> {
    app.state::<Player>()
        .0
        .run_mobile_plugin::<()>("evaluate", serde_json::json!({"script":script}))
        .map_err(|e| e.to_string())
}
#[cfg(target_os = "android")]
pub fn navigate(app: &AppHandle, url: &str) -> Result<(), String> {
    app.state::<Player>()
        .0
        .run_mobile_plugin::<()>("navigate", serde_json::json!({"url":url}))
        .map_err(|e| e.to_string())
}
#[cfg(target_os = "android")]
pub fn account(app: &AppHandle) -> Result<(), String> {
    app.state::<Player>()
        .0
        .run_mobile_plugin::<()>("account", ())
        .map_err(|e| e.to_string())
}
#[cfg(target_os = "android")]
pub fn remote_state(app: &AppHandle, status: serde_json::Value) -> Result<(), String> {
    app.state::<Player>()
        .0
        .run_mobile_plugin::<()>("remoteState", serde_json::json!({"status": status}))
        .map_err(|e| e.to_string())
}
#[cfg(target_os = "android")]
pub fn discovery_multicast(app: &AppHandle, enabled: bool) -> Result<(), String> {
    app.state::<Player>()
        .0
        .run_mobile_plugin::<()>("discoveryMulticast", serde_json::json!({"enabled":enabled}))
        .map_err(|e| e.to_string())
}
