use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager,
};
use ytm_protocol::PlayerCommand;

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let audio_status = MenuItem::with_id(
        app,
        "audio-status",
        "Audio sharing off",
        false,
        None::<&str>,
    )?;
    let audio_mute = MenuItem::with_id(
        app,
        "audio-mute",
        "Mute shared microphone",
        false,
        None::<&str>,
    )?;
    let audio_stop =
        MenuItem::with_id(app, "audio-stop", "Stop audio sharing", false, None::<&str>)?;
    let items = [
        MenuItem::with_id(app, "show", "Open YTM Listen", true, None::<&str>)?,
        MenuItem::with_id(app, "toggle", "Play / Pause", true, None::<&str>)?,
        MenuItem::with_id(app, "next", "Next track", true, None::<&str>)?,
        MenuItem::with_id(app, "previous", "Previous track", true, None::<&str>)?,
        MenuItem::with_id(app, "search", "Quick search", true, None::<&str>)?,
        MenuItem::with_id(app, "mini", "Mini-player", true, None::<&str>)?,
        MenuItem::with_id(app, "party", "Listen together", true, None::<&str>)?,
        MenuItem::with_id(app, "lyrics", "Lyrics", true, None::<&str>)?,
        MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?,
        audio_status.clone(),
        audio_mute.clone(),
        audio_stop.clone(),
        MenuItem::with_id(app, "quit", "Quit YTM Listen", true, None::<&str>)?,
    ];
    let references: Vec<&dyn tauri::menu::IsMenuItem<_>> = items
        .iter()
        .map(|i| i as &dyn tauri::menu::IsMenuItem<_>)
        .collect();
    let menu = Menu::with_items(app, &references)?;
    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("YTM Listen")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "quit" => app.exit(0),
            "audio-stop" | "audio-mute" => {
                let app = app.clone();
                let stop = event.id.as_ref() == "audio-stop";
                tauri::async_runtime::spawn(async move {
                    if stop {
                        let _ = crate::audio::stop_routing(&app).await;
                    } else {
                        let _ = crate::audio::mute_from_tray(&app).await;
                    }
                });
            }
            "show" => {
                let _ = crate::windows::show_music(app);
            }
            "toggle" | "next" | "previous" => {
                let command = match event.id.as_ref() {
                    "next" => PlayerCommand::Next,
                    "previous" => PlayerCommand::Previous,
                    _ => PlayerCommand::Toggle,
                };
                let _ = crate::commands::user_command(app, &command);
            }
            "settings" | "mini" | "party" | "lyrics" | "search" => {
                let app = app.clone();
                let panel = event.id.as_ref().to_string();
                tauri::async_runtime::spawn(async move {
                    let _ = crate::windows::open_panel(&app, &panel);
                });
            }
            _ => (),
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                let _ = crate::windows::show_music(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    let tray = builder.build(app)?;
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut previous = String::new();
        loop {
            let audio = handle.state::<crate::audio::AudioState>().snapshot();
            let live = audio.active && audio.microphone_enabled && !audio.microphone_muted;
            let label = if live {
                "Sharing music + microphone"
            } else if audio.active {
                "Sharing music · microphone off"
            } else {
                "Audio sharing off"
            };
            if label != previous {
                let _ = audio_status.set_text(label);
                let _ = audio_mute.set_enabled(live);
                let _ = audio_stop.set_enabled(audio.active);
                let _ = tray.set_tooltip(Some(format!("YTM Listen · {label}")));
                previous = label.into();
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    });
    Ok(())
}
