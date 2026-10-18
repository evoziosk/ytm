mod catalog;
mod commands;
mod diagnostics;
mod discord;
mod lyrics;
mod media;
mod party;
mod playback;
mod queue;
mod settings;
mod shortcuts;
mod state;
mod tray;
mod windows;

use state::Core;
use tauri::Manager;
use tauri_plugin_deep_link::DeepLinkExt;

pub fn profile() -> String {
    let args: Vec<_> = std::env::args().collect();
    args.iter()
        .position(|arg| arg == "--profile")
        .and_then(|i| args.get(i + 1))
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 32
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
        .cloned()
        .unwrap_or_else(|| "default".into())
}
pub fn receive_link(app: &tauri::AppHandle, link: &str) {
    let Ok(url) = url::Url::parse(link) else {
        return;
    };
    let code = url.path().trim_start_matches('/').to_uppercase();
    if url.scheme() != "ytmlisten"
        || url.host_str() != Some("join")
        || !ytm_protocol::valid_room(&code)
    {
        return;
    }
    if let Some(core) = app.try_state::<Core>() {
        core.data.write().expect("state lock").pending_room = Some(code);
        let handle = app.clone();
        tauri::async_runtime::spawn(async move {
            let _ = windows::open_panel(&handle, "party");
            handle.state::<Core>().emit(&handle);
        });
    }
}
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ytm_listen_lib=info".into()),
        )
        .init();
    let builder = tauri::Builder::default();
    // Named development profiles deliberately permit two clients for sync testing.
    let builder = if profile() == "default" {
        builder.plugin(tauri_plugin_single_instance::init(|app, argv, _| {
            for arg in argv {
                receive_link(app, &arg);
            }
            let _ = windows::show_music(app);
        }))
    } else {
        builder
    };
    builder
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_filename(format!("window-state-{}.json", profile()))
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::SIZE
                        | tauri_plugin_window_state::StateFlags::POSITION
                        | tauri_plugin_window_state::StateFlags::MAXIMIZED,
                )
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            commands::player_event,
            commands::get_snapshot,
            commands::send_command,
            playback::retry_playback,
            playback::dismiss_playback_error,
            queue::queue_action,
            commands::save_settings,
            commands::set_window_blur,
            commands::open_panel,
            commands::show_music,
            commands::party_connect,
            commands::party_leave,
            commands::party_transfer,
            lyrics::get_lyrics,
            catalog::music_query,
            catalog::catalog_result,
            catalog::play_items,
            commands::open_account,
            commands::logout_account,
            commands::refresh_music
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            let settings = settings::load(&handle)?;
            let core = Core::new(settings.clone());
            app.manage(core.clone());
            windows::create_main(&handle)?;
            tray::setup(&handle)?;
            if let Err(error) = shortcuts::apply(&handle, &settings) {
                tracing::warn!(%error, "Some shortcuts are unavailable; edit them in Settings");
            }
            if let Err(error) = media::setup(&handle, &core) {
                tracing::warn!(%error, "Native media controls unavailable");
            }
            discord::start(&handle, &core);
            app.deep_link().on_open_url({
                let handle = handle.clone();
                move |event| {
                    for url in event.urls() {
                        receive_link(&handle, url.as_str());
                    }
                }
            });
            if let Ok(Some(urls)) = app.deep_link().get_current() {
                for url in urls {
                    receive_link(&handle, url.as_str());
                }
            }
            for arg in std::env::args() {
                receive_link(&handle, &arg);
            }
            if !std::env::args().any(|arg| arg == "--check-music") {
                windows::open_panel(&handle, "settings")?;
            }
            diagnostics::start(&handle, &core);
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "search" && matches!(event, tauri::WindowEvent::Focused(false)) {
                let _ = window.hide();
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label().starts_with("auth-") {
                    return;
                }
                let core = window.state::<Core>();
                if window.label() != "settings" || core.snapshot().settings.close_to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                } else {
                    window.app_handle().exit(0);
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("Build YTM Listen")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(core) = app.try_state::<Core>() {
                    party::client::leave(app, &core);
                }
            }
        });
}
