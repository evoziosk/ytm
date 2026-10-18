use crate::settings::Settings;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use ytm_protocol::PlayerCommand;

pub fn apply(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let shortcuts = [
        &settings.hotkey_toggle,
        &settings.hotkey_next,
        &settings.hotkey_previous,
        &settings.hotkey_like,
        &settings.hotkey_mini,
        &settings.hotkey_search,
    ];
    let mut parsed = Vec::new();
    for input in shortcuts {
        if input.trim().is_empty() {
            parsed.push(None);
        } else {
            parsed.push(Some(
                input
                    .parse::<Shortcut>()
                    .map_err(|e| format!("Invalid shortcut {input}: {e}"))?,
            ));
        }
    }
    let mut ids = std::collections::HashSet::new();
    for shortcut in parsed.iter().flatten() {
        if !ids.insert(shortcut.id()) {
            return Err("Each shortcut must be unique".into());
        }
    }
    let mut failures = Vec::new();
    for (index, shortcut) in parsed.into_iter().enumerate() {
        if let Some(shortcut) = shortcut {
            if let Err(error) = register(app, index, shortcut) {
                failures.push(error);
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

fn register(app: &AppHandle, index: usize, shortcut: Shortcut) -> Result<(), String> {
    app.global_shortcut()
        .on_shortcut(shortcut, move |app, _, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }
            if index == 4 || index == 5 {
                let panel = if index == 5 { "search" } else { "mini" };
                if let Some(window) = app.get_webview_window(panel) {
                    if window.is_visible().unwrap_or(false) {
                        let _ = window.hide();
                    } else {
                        let _ = crate::windows::open_panel(app, panel);
                    }
                } else {
                    let handle = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = crate::windows::open_panel(&handle, panel);
                    });
                }
            } else {
                let command = match index {
                    0 => PlayerCommand::Toggle,
                    1 => PlayerCommand::Next,
                    2 => PlayerCommand::Previous,
                    _ => PlayerCommand::Like,
                };
                let _ = crate::commands::user_command(app, &command);
            }
        })
        .map_err(|e| format!("Shortcut unavailable: {e}"))
}

fn keys(settings: &Settings) -> [&str; 6] {
    [
        &settings.hotkey_toggle,
        &settings.hotkey_next,
        &settings.hotkey_previous,
        &settings.hotkey_like,
        &settings.hotkey_mini,
        &settings.hotkey_search,
    ]
}

trait Registrar {
    fn contains(&self, shortcut: Shortcut) -> bool;
    fn unregister(&mut self, shortcut: Shortcut) -> Result<(), String>;
    fn register(&mut self, index: usize, shortcut: Shortcut) -> Result<(), String>;
}
struct NativeRegistrar<'a>(&'a AppHandle);
impl Registrar for NativeRegistrar<'_> {
    fn contains(&self, shortcut: Shortcut) -> bool {
        self.0.global_shortcut().is_registered(shortcut)
    }
    fn unregister(&mut self, shortcut: Shortcut) -> Result<(), String> {
        self.0
            .global_shortcut()
            .unregister(shortcut)
            .map_err(|error| error.to_string())
    }
    fn register(&mut self, index: usize, shortcut: Shortcut) -> Result<(), String> {
        register(self.0, index, shortcut)
    }
}
pub fn update(app: &AppHandle, previous: &Settings, next: &Settings) -> Result<(), String> {
    update_with(&mut NativeRegistrar(app), previous, next)
}
fn update_with(
    registrar: &mut impl Registrar,
    previous: &Settings,
    next: &Settings,
) -> Result<(), String> {
    if keys(previous) == keys(next) {
        return Ok(());
    }
    let parse = |settings: &Settings| -> Result<Vec<Option<Shortcut>>, String> {
        keys(settings)
            .iter()
            .map(|key| {
                if key.trim().is_empty() {
                    Ok(None)
                } else {
                    key.parse()
                        .map(Some)
                        .map_err(|e| format!("Invalid shortcut {key}: {e}"))
                }
            })
            .collect()
    };
    let old = parse(previous)?;
    let new = parse(next)?;
    let mut ids = std::collections::HashSet::new();
    for shortcut in new.iter().flatten() {
        if !ids.insert(shortcut.id()) {
            return Err("Each shortcut must be unique".into());
        }
    }
    let changed: Vec<_> = (0..6)
        .filter(|&i| old[i].as_ref().map(|s| s.id()) != new[i].as_ref().map(|s| s.id()))
        .collect();
    let mut removed = Vec::new();
    for &i in &changed {
        if let Some(shortcut) = old[i] {
            if registrar.contains(shortcut) {
                if let Err(error) = registrar.unregister(shortcut) {
                    for &index in &removed {
                        if let Some(key) = old[index] {
                            let _ = registrar.register(index, key);
                        }
                    }
                    return Err(error.to_string());
                }
                removed.push(i);
            }
        }
    }
    let mut added = Vec::new();
    for &i in &changed {
        if let Some(shortcut) = new[i] {
            if let Err(error) = registrar.register(i, shortcut) {
                for &index in &added {
                    if let Some(key) = new[index] {
                        let _ = registrar.unregister(key);
                    }
                }
                let mut rollback_errors = Vec::new();
                for &index in &removed {
                    if let Some(key) = old[index] {
                        if let Err(error) = registrar.register(index, key) {
                            rollback_errors.push(error);
                        }
                    }
                }
                return Err(if rollback_errors.is_empty() {
                    error
                } else {
                    format!("{error}. Could not restore: {}", rollback_errors.join("; "))
                });
            }
            added.push(i);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    struct Fake {
        live: HashSet<u32>,
        unavailable: u32,
        calls: usize,
    }
    impl Registrar for Fake {
        fn contains(&self, shortcut: Shortcut) -> bool {
            self.live.contains(&shortcut.id())
        }
        fn unregister(&mut self, shortcut: Shortcut) -> Result<(), String> {
            self.calls += 1;
            self.live.remove(&shortcut.id());
            Ok(())
        }
        fn register(&mut self, _: usize, shortcut: Shortcut) -> Result<(), String> {
            self.calls += 1;
            if shortcut.id() == self.unavailable {
                Err("Already used by another app".into())
            } else {
                self.live.insert(shortcut.id());
                Ok(())
            }
        }
    }
    #[test]
    fn unrelated_preferences_save_with_an_unavailable_existing_hotkey() {
        let previous = Settings::default();
        let mut next = previous.clone();
        next.incognito = true;
        let mut fake = Fake {
            live: HashSet::new(),
            unavailable: previous.hotkey_toggle.parse::<Shortcut>().unwrap().id(),
            calls: 0,
        };
        assert!(update_with(&mut fake, &previous, &next).is_ok());
        assert_eq!(fake.calls, 0);
    }
    #[test]
    fn changed_shortcut_failure_restores_registered_keys_and_leaves_others_alone() {
        let previous = Settings::default();
        let mut next = previous.clone();
        next.hotkey_next = "Alt+N".into();
        let live: HashSet<_> = keys(&previous)
            .iter()
            .skip(1)
            .map(|key| key.parse::<Shortcut>().unwrap().id())
            .collect();
        let mut fake = Fake {
            live: live.clone(),
            unavailable: next.hotkey_next.parse::<Shortcut>().unwrap().id(),
            calls: 0,
        };
        assert!(update_with(&mut fake, &previous, &next).is_err());
        assert_eq!(fake.live, live);
        assert_eq!(fake.calls, 3);
    }
}
