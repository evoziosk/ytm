use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle,
};
use ytm_protocol::PlayerCommand;

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
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
    builder.build(app)?;
    Ok(())
}
