use serde::{Deserialize, Serialize};
use tauri::AppHandle;
#[cfg(not(target_os = "android"))]
use tauri::Manager;
#[cfg(not(target_os = "android"))]
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_store::StoreExt;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub close_to_tray: bool,
    pub autostart: bool,
    pub window_blur: bool,
    pub discord_enabled: bool,
    pub discord_app_id: String,
    pub hide_paused: bool,
    pub idle_minutes: u32,
    pub incognito: bool,
    pub blocked_artists: Vec<String>,
    pub blocked_playlists: Vec<String>,
    pub relay_url: String,
    pub display_name: String,
    pub hotkey_toggle: String,
    pub hotkey_next: String,
    pub hotkey_previous: String,
    pub hotkey_like: String,
    pub hotkey_mini: String,
    pub hotkey_search: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            close_to_tray: true,
            autostart: false,
            window_blur: false,
            discord_enabled: true,
            discord_app_id: String::new(),
            hide_paused: true,
            idle_minutes: 5,
            incognito: false,
            blocked_artists: vec![],
            blocked_playlists: vec![],
            relay_url: String::new(),
            display_name: "Listener".into(),
            hotkey_toggle: "Ctrl+Shift+Space".into(),
            hotkey_next: "Ctrl+Shift+ArrowRight".into(),
            hotkey_previous: "Ctrl+Shift+ArrowLeft".into(),
            hotkey_like: "Ctrl+Shift+L".into(),
            hotkey_mini: "Ctrl+Shift+M".into(),
            hotkey_search: "Ctrl+Shift+K".into(),
        }
    }
}
impl Settings {
    pub fn validate(&mut self) -> Result<(), String> {
        self.display_name = self.display_name.trim().to_string();
        if self.display_name.is_empty() || self.display_name.chars().count() > 32 {
            return Err("Display name must have 1–32 characters".into());
        }
        if self.display_name.len() > 32 {
            return Err("Display name must fit in 32 UTF-8 bytes for the room protocol".into());
        }
        if !self.discord_app_id.is_empty()
            && (!(16..=22).contains(&self.discord_app_id.len())
                || !self.discord_app_id.bytes().all(|b| b.is_ascii_digit()))
        {
            return Err("Enter a numeric Discord application ID".into());
        }
        if !self.relay_url.is_empty() {
            let url = url::Url::parse(&self.relay_url).map_err(|_| "Invalid relay URL")?;
            let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
            if !(url.scheme() == "https" || local && url.scheme() == "http")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
                || url.path() != "/"
            {
                return Err("Use an HTTPS relay origin (HTTP is allowed for localhost)".into());
            }
            self.relay_url = url.origin().ascii_serialization();
        }
        if self.idle_minutes > 1440
            || self.blocked_artists.len() > 200
            || self.blocked_playlists.len() > 200
        {
            return Err("Settings exceed the supported limits".into());
        }
        if self
            .blocked_artists
            .iter()
            .chain(&self.blocked_playlists)
            .any(|s| s.len() > 512)
        {
            return Err("A blocklist entry is too long".into());
        }
        self.blocked_artists = self
            .blocked_artists
            .iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect();
        self.blocked_playlists = self
            .blocked_playlists
            .iter()
            .map(|value| playlist_id(value))
            .filter(|value| !value.is_empty())
            .collect();
        for shortcut in [
            &self.hotkey_toggle,
            &self.hotkey_next,
            &self.hotkey_previous,
            &self.hotkey_like,
            &self.hotkey_mini,
            &self.hotkey_search,
        ] {
            if shortcut.len() > 100 {
                return Err("Shortcut is too long".into());
            }
        }
        Ok(())
    }
}
pub fn load(app: &AppHandle) -> Result<Settings, Box<dyn std::error::Error>> {
    let profile = crate::profile();
    let store = app.store(format!("settings-{profile}.json"))?;
    let mut settings: Settings = store
        .get("settings")
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_default();
    settings.validate().map_err(std::io::Error::other)?;
    Ok(settings)
}
#[cfg(not(target_os = "android"))]
pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let previous = app.state::<crate::state::Core>().snapshot().settings;
    let store = app
        .store(format!("settings-{}.json", crate::profile()))
        .map_err(|e| e.to_string())?;
    crate::shortcuts::update(app, &previous, settings)?;
    let manager = app.autolaunch();
    let changed_autostart = settings.autostart != previous.autostart;
    let auto_result = if !changed_autostart {
        Ok(())
    } else if settings.autostart {
        manager.enable()
    } else {
        manager.disable()
    };
    if let Err(error) = auto_result {
        let _ = crate::shortcuts::update(app, settings, &previous);
        return Err(format!("Could not update startup setting: {error}"));
    }
    let result = (|| {
        store.set(
            "settings",
            serde_json::to_value(settings).map_err(|e| e.to_string())?,
        );
        store.save().map_err(|e| e.to_string())
    })();
    if result.is_err() {
        if let Ok(value) = serde_json::to_value(&previous) {
            store.set("settings", value);
        }
        let _ = crate::shortcuts::update(app, settings, &previous);
        if changed_autostart && previous.autostart {
            let _ = manager.enable();
        } else if changed_autostart {
            let _ = manager.disable();
        }
    }
    result
}

/// Accept either a playlist ID, a Music browse ID, or a copied YouTube link.
pub fn playlist_id(value: &str) -> String {
    let trimmed = value.trim();
    let id = url::Url::parse(trimmed)
        .ok()
        .filter(|url| {
            matches!(
                url.host_str(),
                Some("music.youtube.com" | "www.youtube.com" | "youtube.com")
            )
        })
        .and_then(|url| {
            url.query_pairs()
                .find(|(key, _)| key == "list")
                .map(|(_, value)| value.into_owned())
        })
        .unwrap_or_else(|| trimmed.to_string());
    id.strip_prefix("VL").unwrap_or(&id).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn existing_settings_gain_the_quick_search_shortcut() {
        let settings: Settings =
            serde_json::from_value(serde_json::json!({ "incognito": true, "hotkey_toggle": "" }))
                .unwrap();
        assert_eq!(settings.hotkey_search, "Ctrl+Shift+K");
        assert!(settings.incognito);
        assert!(settings.hotkey_toggle.is_empty());
        assert!(!settings.window_blur);
    }
    #[test]
    fn normalizes_copied_playlist_links_and_browse_ids() {
        assert_eq!(playlist_id("VLPLabc_123"), "PLabc_123");
        assert_eq!(
            playlist_id("https://music.youtube.com/playlist?list=PLabc_123"),
            "PLabc_123"
        );
        let mut settings = Settings {
            blocked_playlists: vec!["  ".into(), "VLPLabc_123".into()],
            ..Default::default()
        };
        settings.validate().unwrap();
        assert_eq!(settings.blocked_playlists, ["PLabc_123"]);
    }
}

#[cfg(target_os = "android")]
pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let store = app
        .store(format!("settings-{}.json", crate::profile()))
        .map_err(|e| e.to_string())?;
    store.set(
        "settings",
        serde_json::to_value(settings).map_err(|e| e.to_string())?,
    );
    store.save().map_err(|e| e.to_string())
}
