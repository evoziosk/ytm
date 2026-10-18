use std::sync::atomic::{AtomicUsize, Ordering};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_opener::OpenerExt;
static AUTH_ID: AtomicUsize = AtomicUsize::new(0);

fn player_directory(app: &AppHandle) -> tauri::Result<std::path::PathBuf> {
    Ok(app
        .path()
        .app_data_dir()?
        .join("profiles")
        .join(crate::profile()))
}

pub fn allowed(url: &url::Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && url.host_str().is_some_and(|host| {
            host == "youtube.com"
                || host.ends_with(".youtube.com")
                || matches!(
                    host,
                    "accounts.google.com"
                        | "consent.google.com"
                        | "myaccount.google.com"
                        | "accounts.google.com.pk"
                        | "accounts.google.co.uk"
                        | "consent.google.com.pk"
                )
                || matches!(
                    host,
                    "www.google.com" | "www.google.com.pk" | "www.google.co.uk"
                ) && url.path().starts_with("/accounts/")
        })
}
fn account_navigation(app: &AppHandle, url: &url::Url) -> bool {
    if allowed(url) || url.as_str() == "about:blank" {
        return true;
    }
    // Never send a partially completed login redirect to a different browser/profile.
    let core = app.state::<crate::state::Core>();
    core.data.write().expect("state lock").account_message = format!("Sign-in stopped at an unsupported destination: {}. No account data was sent to another browser.", url.host_str().unwrap_or("unknown host"));
    core.emit(app);
    false
}
fn account_loaded(window: tauri::WebviewWindow, event: tauri::webview::PageLoadPayload<'_>) {
    if matches!(event.event(), tauri::webview::PageLoadEvent::Finished)
        && (event.url().host_str() == Some("music.youtube.com")
            || window.label() == "auth-logout"
                && matches!(
                    event.url().host_str(),
                    Some("www.youtube.com" | "youtube.com")
                ))
        && !event.url().path().starts_with("/logout")
    {
        if let Some(player) = window.app_handle().get_webview_window("main") {
            let _ = player.navigate("https://music.youtube.com".parse().expect("static URL"));
        }
        let _ = window.close();
    }
}
fn popup(
    app: &AppHandle,
    url: url::Url,
    features: tauri::webview::NewWindowFeatures,
) -> tauri::webview::NewWindowResponse<tauri::Wry> {
    if !allowed(&url)
        || app
            .webview_windows()
            .keys()
            .filter(|label| label.starts_with("auth-"))
            .count()
            >= 4
    {
        return tauri::webview::NewWindowResponse::Deny;
    }
    let label = format!("auth-{}", AUTH_ID.fetch_add(1, Ordering::Relaxed));
    let nav = app.clone();
    let child = app.clone();
    let builder = WebviewWindowBuilder::new(
        app,
        label,
        WebviewUrl::External("about:blank".parse().expect("static URL")),
    )
    .window_features(features)
    .title("Sign in · YTM Listen")
    .inner_size(560.0, 760.0)
    .on_navigation(move |url| account_navigation(&nav, url))
    .on_new_window(move |url, features| popup(&child, url, features))
    .on_page_load(account_loaded);
    match builder.build() {
        Ok(window) => tauri::webview::NewWindowResponse::Create { window },
        Err(error) => {
            tracing::warn!(%error, "Could not open account popup");
            tauri::webview::NewWindowResponse::Deny
        }
    }
}
pub fn open_account(app: &AppHandle) -> Result<(), String> {
    account_window(app, "https://accounts.google.com/ServiceLogin?service=youtube&continue=https%3A%2F%2Fmusic.youtube.com%2F")
}
pub fn logout_account(app: &AppHandle) -> Result<(), String> {
    for (label, window) in app.webview_windows() {
        if label.starts_with("auth-") && label != "auth-logout" {
            let _ = window.close();
        }
    }
    account_window(app, "https://music.youtube.com/logout")
}
fn account_window(app: &AppHandle, destination: &str) -> Result<(), String> {
    let label = if destination.ends_with("/logout") {
        "auth-logout"
    } else {
        "auth-account"
    };
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.show();
        return window.set_focus().map_err(|e| e.to_string());
    }
    let nav = app.clone();
    let child = app.clone();
    let builder = WebviewWindowBuilder::new(
        app,
        label,
        WebviewUrl::External(destination.parse().map_err(|_| "Invalid account URL")?),
    )
    .data_directory(player_directory(app).map_err(|e| e.to_string())?)
    .title("Google account · YTM Listen")
    .inner_size(560.0, 760.0)
    .on_navigation(move |url| account_navigation(&nav, url))
    .on_new_window(move |url, features| popup(&child, url, features))
    .on_page_load(account_loaded);
    #[cfg(target_os = "windows")]
    let builder = builder.additional_browser_args(
        "--disable-features=HardwareMediaKeyHandling --autoplay-policy=no-user-gesture-required",
    );
    builder.build().map_err(|e| e.to_string())?;
    Ok(())
}
pub fn create_main(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let opener = app.clone();
    let popup = app.clone();
    let directory = player_directory(app)?;
    std::fs::create_dir_all(&directory)?;
    let builder = WebviewWindowBuilder::new(
        app,
        "main",
        WebviewUrl::External("https://music.youtube.com".parse()?),
    )
    .title("YouTube Music · YTM Listen")
    .inner_size(1240.0, 820.0)
    .min_inner_size(640.0, 480.0)
    .visible(false)
    .data_directory(directory)
    .initialization_script(include_str!("../../bridge/dist/bridge.js"))
    .on_page_load(|window, event| {
        if matches!(event.event(), tauri::webview::PageLoadEvent::Started) {
            let core = window.state::<crate::state::Core>();
            for (_, sender) in core.command_pending.lock().expect("command lock").drain() {
                let _ = sender.send(Err("Playback connection changed. Please retry.".into()));
            }
            for (_, sender) in core.catalog_pending.lock().expect("catalog lock").drain() {
                let _ = sender.send(Err("Music connection changed. Please retry.".into()));
            }
            {
                let mut data = core.data.write().expect("state lock");
                data.player = Default::default();
                data.bridge_connected = false;
            }
            let _ = core.changes.send(Default::default());
            core.emit(window.app_handle());
        }
    })
    .on_navigation(move |url| {
        if allowed(url) {
            return true;
        }
        if url.host_str().is_some_and(|host| {
            host.starts_with("accounts.google.") || host.starts_with("consent.google.")
        }) || url.path().starts_with("/accounts/")
        {
            return account_navigation(&opener, url);
        }
        if matches!(url.scheme(), "https" | "http") {
            let _ = opener.opener().open_url(url.as_str(), None::<&str>);
        }
        false
    })
    .on_new_window(move |url, features| self::popup(&popup, url, features));
    #[cfg(target_os = "windows")]
    let builder = builder.additional_browser_args(
        "--disable-features=HardwareMediaKeyHandling --autoplay-policy=no-user-gesture-required",
    );
    builder.build()?;
    Ok(())
}
pub fn show_music(app: &AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("settings")
        .ok_or("Music app window unavailable")?;
    window.show().map_err(|e| e.to_string())?;
    let _ = window.unminimize();
    window.set_focus().map_err(|e| e.to_string())
}
pub fn open_panel(app: &AppHandle, panel: &str) -> Result<(), String> {
    if !["settings", "mini", "party", "lyrics", "search"].contains(&panel) {
        return Err("Unknown panel".into());
    }
    if let Some(window) = app.get_webview_window(panel) {
        if panel == "search" {
            position_search(app, &window);
        }
        window.show().map_err(|e| e.to_string())?;
        return window.set_focus().map_err(|e| e.to_string());
    }
    let (width, height) = match panel {
        "mini" => (390.0, 220.0),
        "party" => (960.0, 740.0),
        "lyrics" => (480.0, 680.0),
        "search" => (660.0, 480.0),
        _ => (1060.0, 780.0),
    };
    let builder = WebviewWindowBuilder::new(
        app,
        panel,
        WebviewUrl::App(format!("index.html?panel={panel}").into()),
    )
    .title(if panel == "settings" {
        "YTM Listen".to_string()
    } else {
        format!("YTM Listen · {panel}")
    })
    .inner_size(width, height)
    .min_inner_size(
        if panel == "search" {
            420.0
        } else if panel == "mini" {
            350.0
        } else if panel == "lyrics" {
            360.0
        } else {
            760.0
        },
        if panel == "search" {
            300.0
        } else if panel == "mini" {
            200.0
        } else {
            500.0
        },
    )
    .always_on_top(panel == "mini" || panel == "search")
    .decorations(panel != "mini" && panel != "search")
    .resizable(panel != "mini" && panel != "search")
    .skip_taskbar(panel == "search")
    .visible(panel != "search")
    .on_navigation(|url| {
        url.scheme() == "tauri"
            || url.host_str() == Some("tauri.localhost")
            || cfg!(debug_assertions)
                && matches!(url.host_str(), Some("localhost" | "127.0.0.1"))
                && url.port() == Some(1420)
    });
    // Main-window transparency is created once; opaque mode is supplied by the
    // page background so Acrylic can be switched without recreating the session.
    let builder = if panel == "search" || panel == "settings" {
        builder
            .transparent(true)
            .background_color(tauri::window::Color(0, 0, 0, 0))
    } else {
        builder
    };
    #[cfg(target_os = "windows")]
    let builder = if panel == "search"
        || panel == "settings"
            && app
                .state::<crate::state::Core>()
                .data
                .read()
                .expect("state lock")
                .settings
                .window_blur
    {
        builder
            .theme(Some(tauri::Theme::Dark))
            .effects(blur_effect())
    } else {
        builder
    };
    let window = builder.build().map_err(|e| e.to_string())?;
    if panel == "search" {
        position_search(app, &window);
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn blur_effect() -> tauri::utils::config::WindowEffectsConfig {
    tauri::window::EffectsBuilder::new()
        .effect(tauri::window::Effect::Acrylic)
        .color(tauri::window::Color(15, 22, 19, 80))
        .build()
}

pub fn set_app_blur(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let window = app
        .get_webview_window("settings")
        .ok_or("Main app window unavailable")?;
    #[cfg(target_os = "windows")]
    {
        window
            .set_theme(if enabled {
                Some(tauri::Theme::Dark)
            } else {
                None
            })
            .map_err(|error| error.to_string())?;
        window
            .set_effects(if enabled { Some(blur_effect()) } else { None })
            .map_err(|error| error.to_string())?;
    }
    #[cfg(not(target_os = "windows"))]
    let _ = (window, enabled);
    Ok(())
}

fn position_search(app: &AppHandle, window: &tauri::WebviewWindow) {
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|cursor| app.monitor_from_point(cursor.x, cursor.y).ok().flatten());
    if let (Some(monitor), Ok(size)) = (monitor, window.outer_size()) {
        let area = monitor.work_area();
        let x = area.position.x + (area.size.width as i32 - size.width as i32).max(0) / 2;
        let y = area.position.y + (area.size.height as i32 - size.height as i32).max(0) / 2;
        let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
    } else {
        let _ = window.center();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn navigation_allowlist_rejects_lookalikes() {
        assert!(allowed(
            &"https://music.youtube.com/watch?v=x".parse().unwrap()
        ));
        assert!(allowed(
            &"https://accounts.google.com/signin".parse().unwrap()
        ));
        assert!(allowed(
            &"https://accounts.google.com.pk/accounts/SetSID"
                .parse()
                .unwrap()
        ));
        assert!(allowed(
            &"https://www.google.com.pk/accounts/SetSID".parse().unwrap()
        ));
        for s in [
            "https://youtube.com.evil.test",
            "http://music.youtube.com",
            "https://evilyoutube.com",
            "https://accounts.google.com:444",
            "https://user@music.youtube.com",
            "https://accounts.google.com.pk.evil.test/accounts/SetSID",
            "https://www.google.com/search?q=test",
        ] {
            assert!(!allowed(&s.parse().unwrap()));
        }
    }
}
