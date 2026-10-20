//! Playback session access, independent of desktop window management.
use tauri::AppHandle;
#[cfg(not(target_os = "android"))]
use tauri::Manager;

pub fn evaluate(app: &AppHandle, script: &str) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        tauri_plugin_ytm_player::evaluate(app, script)
    }
    #[cfg(not(target_os = "android"))]
    {
        let main = app
            .get_webview_window("main")
            .ok_or("Playback service unavailable")?;
        if main
            .url()
            .map_err(|e| e.to_string())?
            .origin()
            .ascii_serialization()
            != "https://music.youtube.com"
        {
            return Err("Finish signing in, then return to YouTube Music".into());
        }
        main.eval(script).map_err(|e| e.to_string())
    }
}
pub fn refresh(app: &AppHandle) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        tauri_plugin_ytm_player::navigate(app, "https://music.youtube.com")
    }
    #[cfg(not(target_os = "android"))]
    {
        app.get_webview_window("main")
            .ok_or("Playback service unavailable")?
            .navigate("https://music.youtube.com".parse().expect("static URL"))
            .map_err(|e| e.to_string())
    }
}
