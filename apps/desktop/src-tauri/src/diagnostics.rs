use crate::{catalog, commands, state::Core};
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};
use ytm_protocol::{CatalogKind, CatalogRequest, PlayerCommand};

/// Explicit development check against the real playback WebView. No session secrets are exported.
pub fn start(app: &AppHandle, core: &Core) {
    #[cfg(target_os = "windows")]
    if std::env::args().any(|arg| arg == "--check-audio-routing") {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(8)).await;
            match crate::audio::check(&app).await {
                Ok(result) => {
                    println!("AUDIO_ROUTING_CHECK {result}");
                    app.exit(if result["passed"] == true { 0 } else { 1 });
                }
                Err(error) => {
                    println!(
                        "AUDIO_ROUTING_CHECK {}",
                        serde_json::json!({"passed":false,"error":error})
                    );
                    app.exit(1);
                }
            }
        });
        return;
    }
    if std::env::args().any(|arg| arg == "--check-control") {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let result = crate::control_smoke::run(&app).await;
            println!(
                "CONTROL_CHECK {}",
                serde_json::json!({"passed":result.is_ok(),"error":result.as_ref().err()})
            );
            app.exit(if result.is_ok() { 0 } else { 1 });
        });
        return;
    }
    if !std::env::args().any(|arg| arg == "--check-music") {
        return;
    }
    let app = app.clone();
    let core = core.clone();
    tauri::async_runtime::spawn(async move {
        if std::env::args().any(|arg| arg == "--check-search") {
            let result = crate::windows::open_panel(&app, "search");
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let window = app.get_webview_window("search");
            let registered = core
                .snapshot()
                .settings
                .hotkey_search
                .parse::<Shortcut>()
                .ok()
                .is_some_and(|key| app.global_shortcut().is_registered(key));
            let visible = window
                .as_ref()
                .is_some_and(|window| window.is_visible().unwrap_or(false));
            let top = window
                .as_ref()
                .is_some_and(|window| window.is_always_on_top().unwrap_or(false));
            let decorated = window
                .as_ref()
                .is_some_and(|window| window.is_decorated().unwrap_or(false));
            let centered = window.as_ref().is_some_and(|window| {
                match (
                    window.outer_position(),
                    window.outer_size(),
                    window.current_monitor(),
                ) {
                    (Ok(position), Ok(size), Ok(Some(monitor))) => {
                        let area = monitor.work_area();
                        let x = area.position.x
                            + (area.size.width as i32 - size.width as i32).max(0) / 2;
                        let y = area.position.y
                            + (area.size.height as i32 - size.height as i32).max(0) / 2;
                        (position.x - x).abs() <= 2 && (position.y - y).abs() <= 2
                    }
                    _ => false,
                }
            });
            if let Some(window) = window {
                let _ = window.hide();
            }
            println!(
                "QUICK_SEARCH_CHECK {}",
                serde_json::json!({ "error": result.err(), "registered": registered, "visible": visible, "always_on_top": top, "decorated": decorated, "centered": centered })
            );
            if !registered || !visible || !top || decorated || !centered {
                app.exit(1);
                return;
            }
        }
        let result = catalog::query(
            &app,
            &core,
            CatalogRequest {
                kind: CatalogKind::Search,
                query: "Daft Punk Get Lucky".into(),
                browse_id: String::new(),
                continuation: String::new(),
            },
        )
        .await;
        match result {
            Ok(page) => {
                if std::env::args().any(|arg| arg == "--check-blur") {
                    match check_blur(&app, &core).await {
                        Ok(()) => println!(
                            "BLUR_CHECK {}",
                            serde_json::json!({"enabled_and_saved": true, "disabled_and_saved": true, "restored_on_reopen": true, "player_preserved": true})
                        ),
                        Err(error) => {
                            println!("BLUR_CHECK {}", serde_json::json!({"error": error}));
                            app.exit(1);
                            return;
                        }
                    }
                }
                let more = if page.continuation.is_empty() {
                    None
                } else {
                    Some(
                        catalog::query(
                            &app,
                            &core,
                            CatalogRequest {
                                kind: CatalogKind::Search,
                                query: "Daft Punk Get Lucky".into(),
                                browse_id: String::new(),
                                continuation: page.continuation.clone(),
                            },
                        )
                        .await,
                    )
                };
                let mut command_error = None;
                let home = catalog::query(
                    &app,
                    &core,
                    CatalogRequest {
                        kind: CatalogKind::Home,
                        query: String::new(),
                        browse_id: String::new(),
                        continuation: String::new(),
                    },
                )
                .await;
                let collection_id = page
                    .items
                    .iter()
                    .chain(home.as_ref().ok().into_iter().flat_map(|p| p.items.iter()))
                    .find(|item| {
                        !item.browse_id.is_empty()
                            && matches!(item.kind.as_str(), "album" | "playlist")
                    })
                    .map(|item| item.browse_id.clone());
                let collection = if let Some(browse_id) = collection_id {
                    Some(
                        catalog::query(
                            &app,
                            &core,
                            CatalogRequest {
                                kind: CatalogKind::Browse,
                                query: String::new(),
                                browse_id,
                                continuation: String::new(),
                            },
                        )
                        .await,
                    )
                } else {
                    None
                };
                let first = page.items.iter().find(|item| !item.video_id.is_empty());
                let mut playback = false;
                let mut seek_checks = Vec::new();
                if std::env::args().any(|arg| arg == "--check-playback") {
                    if let Some(item) = first {
                        command_error = crate::playback::confirmed(
                            &app,
                            PlayerCommand::LoadTrack {
                                video_id: item.video_id.clone(),
                            },
                        )
                        .await
                        .err();
                        for _ in 0..120 {
                            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                            let state = core.snapshot().player;
                            if state
                                .track
                                .as_ref()
                                .is_some_and(|track| track.video_id == item.video_id)
                                && state.playing
                                && !state.ad
                                && state.position >= 1.0
                            {
                                playback = true;
                                break;
                            }
                        }
                        if playback && std::env::args().any(|arg| arg == "--check-seek") {
                            for target in [89.0, 1.0, 129.0] {
                                let started = std::time::Instant::now();
                                let request_error = commands::perform_user_command(
                                    &app,
                                    PlayerCommand::Seek { position: target },
                                )
                                .await
                                .err();
                                let mut acknowledged = false;
                                for _ in 0..320 {
                                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                                    let state = core.snapshot().player;
                                    if !state.ad && (state.position - target).abs() < 1.5 {
                                        acknowledged = true;
                                        break;
                                    }
                                }
                                let ack_ms = started.elapsed().as_millis();
                                tokio::time::sleep(std::time::Duration::from_millis(750)).await;
                                let state = core.snapshot().player;
                                let stable = acknowledged
                                    && !state.ad
                                    && state.position >= target - 1.0
                                    && state.position < target + 5.0;
                                seek_checks.push(serde_json::json!({"target": target, "command_error": request_error, "ack_ms": ack_ms, "stable": stable, "position": state.position}));
                            }
                        }
                        if let Err(error) =
                            crate::playback::confirmed(&app, PlayerCommand::Pause).await
                        {
                            command_error = Some(error);
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    }
                }
                #[cfg(target_os = "windows")]
                if std::env::args().any(|arg| arg == "--check-audio-icon") {
                    match check_audio_icon(&app).await {
                        Ok(count) => println!(
                            "AUDIO_ICON_CHECK {}",
                            serde_json::json!({"sessions": count, "name": "YTM Listen", "icon_matches": true})
                        ),
                        Err(error) => {
                            println!("AUDIO_ICON_CHECK {}", serde_json::json!({"error": error}));
                            app.exit(1);
                            return;
                        }
                    }
                }
                println!(
                    "SEEK_CHECK {}",
                    serde_json::json!({"checks": seek_checks, "audio": core.snapshot().audio})
                );
                println!(
                    "MUSIC_CHECK {}",
                    serde_json::json!({ "command_error": command_error, "next_page_items": more.as_ref().and_then(|result| result.as_ref().ok().map(|page| page.items.len())), "next_page_error": more.as_ref().and_then(|result| result.as_ref().err()), "results": page.items.len(), "songs": page.items.iter().filter(|item| item.kind == "song").count(), "videos": page.items.iter().filter(|item| item.kind == "video").count(), "home_items": home.as_ref().map(|p| p.items.len()).ok(), "home_error": home.as_ref().err(), "collection_items": collection.as_ref().and_then(|result| result.as_ref().ok().map(|p| p.items.len())), "collection_error": collection.as_ref().and_then(|result| result.as_ref().err()), "signed_in": page.signed_in, "playback_progressed": playback, "requested_id": first.map(|item| &item.video_id), "actual_id": core.snapshot().player.track.map(|track| track.video_id), "playing": core.snapshot().player.playing, "buffering": core.snapshot().player.buffering, "ad": core.snapshot().player.ad, "position": core.snapshot().player.position, "duration": core.snapshot().player.duration })
                );
                app.exit(
                    if page.items.is_empty()
                        || command_error.is_some()
                        || more.as_ref().is_some_and(|page| page.is_err())
                        || std::env::args().any(|arg| arg == "--check-playback") && !playback
                        || std::env::args().any(|arg| arg == "--check-seek")
                            && (seek_checks.len() != 3
                                || seek_checks.iter().any(|check| {
                                    check["stable"] != true || !check["command_error"].is_null()
                                }))
                    {
                        1
                    } else {
                        0
                    },
                );
            }
            Err(error) => {
                let location = app
                    .get_webview_window("main")
                    .and_then(|window| window.url().ok())
                    .map(|url| format!("{}{}", url.origin().ascii_serialization(), url.path()));
                println!(
                    "MUSIC_CHECK {}",
                    serde_json::json!({ "error": error, "location": location, "bridge_connected": core.snapshot().bridge_connected })
                );
                app.exit(1);
            }
        }
    });
}

#[cfg(target_os = "windows")]
async fn check_audio_icon(app: &AppHandle) -> Result<usize, String> {
    let mut failure = "No playback audio session found".to_string();
    for _ in 0..20 {
        match crate::media::audio_identity::inspect(app).await {
            Ok(sessions) if !sessions.is_empty() => {
                if sessions
                    .iter()
                    .all(|session| session.name == "YTM Listen" && session.icon_matches)
                {
                    return Ok(sessions.len());
                }
                failure = "Windows audio session name or icon did not match the app".into();
            }
            Err(error) => failure = error,
            _ => {}
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    Err(failure)
}

async fn check_blur(app: &AppHandle, core: &Core) -> Result<(), String> {
    let previous = core.data.read().expect("state lock").settings.window_blur;
    let player = app.get_webview_window("main").ok_or("Player unavailable")?;
    let player_url = player.url().map_err(|error| error.to_string())?;
    crate::windows::open_panel(app, "settings")?;
    crate::commands::commit_settings(app, core, |mut settings| {
        settings.window_blur = true;
        settings
    })?;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    if !crate::settings::load(app)
        .map_err(|error| error.to_string())?
        .window_blur
    {
        return Err("Blur preference was not saved".into());
    }
    app.get_webview_window("settings")
        .ok_or("App window unavailable")?
        .destroy()
        .map_err(|error| error.to_string())?;
    // Window destruction is dispatched to the UI thread. Wait for the old
    // label to leave the manager before creating its replacement.
    for _ in 0..100 {
        if app.get_webview_window("settings").is_none() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    if app.get_webview_window("settings").is_some() {
        return Err("App window did not finish closing".into());
    }
    crate::windows::open_panel(app, "settings")?;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    crate::commands::commit_settings(app, core, |mut settings| {
        settings.window_blur = false;
        settings
    })?;
    if crate::settings::load(app)
        .map_err(|error| error.to_string())?
        .window_blur
    {
        return Err("Opaque appearance was not saved".into());
    }
    crate::commands::commit_settings(app, core, |mut settings| {
        settings.window_blur = previous;
        settings
    })?;
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.hide();
    }
    if player.url().map_err(|error| error.to_string())? != player_url {
        return Err("Playback service navigated while changing appearance".into());
    }
    Ok(())
}
