use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State, WebviewWindow};

#[cfg(any(target_os = "windows", test))]
mod dsp;
#[cfg(target_os = "windows")]
mod native;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub output_id: String,
    pub microphone_id: String,
    pub music_gain: f32,
    pub microphone_gain: f32,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            output_id: String::new(),
            microphone_id: String::new(),
            music_gain: 0.8,
            microphone_gain: 1.0,
        }
    }
}
impl Preferences {
    pub fn validate(&self) -> Result<(), String> {
        if self.output_id.len() > 1024
            || self.microphone_id.len() > 1024
            || !self.music_gain.is_finite()
            || !(0.0..=1.0).contains(&self.music_gain)
            || !self.microphone_gain.is_finite()
            || !(0.0..=2.0).contains(&self.microphone_gain)
        {
            return Err("Invalid virtual microphone settings".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Default)]
pub struct Status {
    pub supported: bool,
    pub active: bool,
    pub microphone_enabled: bool,
    pub microphone_muted: bool,
    pub message: String,
    pub music_peak: f32,
    pub microphone_peak: f32,
    pub output_peak: f32,
    pub limited: bool,
    pub underruns: u64,
    pub dropped_frames: u64,
    pub buffered_ms: u32,
    pub preferences: Preferences,
}
#[derive(Clone, Serialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub virtual_cable: bool,
}
#[derive(Serialize, Default)]
pub struct Devices {
    pub outputs: Vec<Device>,
    pub microphones: Vec<Device>,
}
#[derive(Clone, Deserialize)]
pub struct Routing {
    pub preferences: Preferences,
    pub microphone_enabled: bool,
    pub microphone_muted: bool,
    #[serde(default)]
    pub confirm_microphone: bool,
}
#[cfg(any(target_os = "windows", test))]
pub fn needs_microphone_confirmation(previous: &Routing, next: &Routing) -> bool {
    next.microphone_enabled
        && !next.microphone_muted
        && (!previous.microphone_enabled
            || previous.microphone_muted
            || previous.preferences.microphone_id != next.preferences.microphone_id)
        && !next.confirm_microphone
}

pub struct AudioState {
    pub status: Arc<Mutex<Status>>,
    #[cfg(target_os = "windows")]
    worker: Mutex<Option<native::Worker>>,
    pub transition: tokio::sync::Mutex<()>,
}
impl AudioState {
    pub fn new(preferences: Preferences) -> Self {
        Self {
            status: Arc::new(Mutex::new(Status {
                supported: supported(),
                preferences,
                message: if supported() {
                    "Ready to share music".into()
                } else {
                    "Virtual microphone requires Windows 11 or Windows build 20348 or newer".into()
                },
                ..Default::default()
            })),
            #[cfg(target_os = "windows")]
            worker: Mutex::new(None),
            transition: tokio::sync::Mutex::new(()),
        }
    }
    pub fn snapshot(&self) -> Status {
        self.status.lock().expect("audio status").clone()
    }
    pub fn stop(&self) {
        #[cfg(target_os = "windows")]
        if let Some(worker) = self.worker.lock().expect("audio worker").take() {
            worker.stop();
        }
        let mut status = self.status.lock().expect("audio status");
        status.active = false;
        status.microphone_enabled = false;
        status.microphone_muted = false;
        status.music_peak = 0.0;
        status.microphone_peak = 0.0;
        status.output_peak = 0.0;
        status.buffered_ms = 0;
        status.message = "Music sharing stopped".into();
    }
}
pub fn supported() -> bool {
    #[cfg(target_os = "windows")]
    {
        native::supported()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}
fn local(window: &WebviewWindow) -> Result<(), String> {
    if window.label() != "settings" {
        return Err("Open virtual microphone controls in Preferences".into());
    }
    Ok(())
}
#[tauri::command]
pub fn audio_status(window: WebviewWindow, audio: State<AudioState>) -> Result<Status, String> {
    local(&window)?;
    Ok(audio.snapshot())
}
#[tauri::command]
pub async fn audio_devices(window: WebviewWindow) -> Result<Devices, String> {
    local(&window)?;
    #[cfg(target_os = "windows")]
    {
        tokio::task::spawn_blocking(native::devices)
            .await
            .map_err(|e| e.to_string())?
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(Devices::default())
    }
}
#[tauri::command]
pub async fn audio_stop(app: AppHandle, window: WebviewWindow) -> Result<Status, String> {
    local(&window)?;
    stop_routing(&app).await
}
pub async fn stop_routing(app: &AppHandle) -> Result<Status, String> {
    let audio = app.state::<AudioState>();
    let _transition = audio.transition.lock().await;
    let handle = app.clone();
    tokio::task::spawn_blocking(move || handle.state::<AudioState>().stop())
        .await
        .map_err(|e| e.to_string())?;
    Ok(audio.snapshot())
}

#[cfg(not(target_os = "android"))]
pub async fn mute_from_tray(app: &AppHandle) -> Result<(), String> {
    let audio = app.state::<AudioState>();
    let _transition = audio.transition.lock().await;
    #[cfg(target_os = "windows")]
    {
        let snapshot = audio.snapshot();
        if !snapshot.active || !snapshot.microphone_enabled {
            return Ok(());
        }
        let app = app.clone();
        tokio::task::spawn_blocking(move || {
            let audio = app.state::<AudioState>();
            let worker = audio.worker.lock().expect("audio worker");
            worker
                .as_ref()
                .ok_or("Music sharing is stopped")?
                .apply(Routing {
                    preferences: snapshot.preferences,
                    microphone_enabled: true,
                    microphone_muted: true,
                    confirm_microphone: false,
                })
        })
        .await
        .map_err(|e| e.to_string())??;
    }
    Ok(())
}
#[tauri::command]
pub async fn audio_apply(
    app: AppHandle,
    window: WebviewWindow,
    routing: Routing,
    start: bool,
) -> Result<Status, String> {
    local(&window)?;
    routing.preferences.validate()?;
    if start && routing.microphone_enabled {
        return Err("Start music sharing first, then enable your microphone".into());
    }
    #[cfg(not(target_os = "windows"))]
    let _ = (routing.microphone_muted, routing.confirm_microphone);
    let audio = app.state::<AudioState>();
    let _transition = audio.transition.lock().await;
    if !supported() {
        return Err(audio.snapshot().message);
    }
    #[cfg(target_os = "windows")]
    {
        // Muting/stopping a mic must not depend on a responsive playback page.
        let pid = if start {
            native::playback_pid(&app).await?
        } else {
            0
        };
        let handle = app.clone();
        let next = routing.clone();
        tokio::task::spawn_blocking(move || {
            let audio = handle.state::<AudioState>();
            let mut worker = audio.worker.lock().expect("audio worker");
            if start {
                if audio.snapshot().active {
                    return Err("Music sharing is already running".into());
                }
                if let Some(previous) = worker.take() {
                    previous.stop();
                }
                *worker = Some(native::Worker::start(pid, next, audio.status.clone())?);
            } else {
                worker
                    .as_ref()
                    .ok_or("Music sharing is stopped")?
                    .apply(next)?;
            }
            Ok::<_, String>(())
        })
        .await
        .map_err(|e| e.to_string())??;
        // Persist device/level choices separately. Active capture is transient.
        if let Err(error) = save_preferences(&app, &routing.preferences) {
            return Err(format!(
                "Audio routing applied, but preferences could not be saved: {error}"
            ));
        }
    }
    Ok(audio.snapshot())
}
#[cfg(target_os = "windows")]
fn save_preferences(app: &AppHandle, preferences: &Preferences) -> Result<(), String> {
    use tauri_plugin_store::StoreExt;
    let store = app
        .store(format!("audio-{}.json", crate::profile()))
        .map_err(|e| e.to_string())?;
    store.set(
        "preferences",
        serde_json::to_value(preferences).map_err(|e| e.to_string())?,
    );
    store.save().map_err(|e| e.to_string())
}
pub fn setup(app: &AppHandle) {
    use tauri_plugin_store::StoreExt;
    let preferences = app
        .store(format!("audio-{}.json", crate::profile()))
        .ok()
        .and_then(|store| store.get("preferences"))
        .and_then(|value| serde_json::from_value::<Preferences>(value).ok())
        .filter(|value| value.validate().is_ok())
        .unwrap_or_default();
    app.manage(AudioState::new(preferences));
    #[cfg(target_os = "windows")]
    {
        let handle = app.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                let audio = handle.state::<AudioState>();
                if !audio.snapshot().active {
                    continue;
                }
                let pid = native::playback_pid(&handle).await;
                let changed = audio
                    .worker
                    .lock()
                    .expect("audio worker")
                    .as_ref()
                    .is_some_and(|worker| pid.as_ref().ok() != Some(&worker.pid));
                if changed {
                    let _transition = audio.transition.lock().await;
                    let app = handle.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        let audio = app.state::<AudioState>();
                        audio.stop();
                        audio.status.lock().expect("audio status").message = "Playback process changed. Start sharing again when the player is ready.".into();
                    }).await;
                }
            }
        });
    }
}

/// Explicit developer check: synthetic tones only, no microphone or recording.
#[cfg(target_os = "windows")]
pub async fn check(app: &AppHandle) -> Result<serde_json::Value, String> {
    let pid = native::playback_pid(app).await?;
    let (sender, ready) = tokio::sync::oneshot::channel();
    let capture = tokio::task::spawn_blocking(move || native::probe(pid, sender));
    ready.await.map_err(|e| e.to_string())??;
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    app.get_webview_window("main").ok_or("Playback missing")?.eval(
        "(()=>{const c=new AudioContext();const o=c.createOscillator();const g=c.createGain();o.frequency.value=880;g.gain.value=0.02;o.connect(g).connect(c.destination);c.resume().then(()=>{o.start();o.stop(c.currentTime+1.5);o.onended=()=>c.close();});})()"
    ).map_err(|e| e.to_string())?;
    capture.await.map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_levels_are_rejected_and_capture_is_not_persisted() {
        let mut preferences = Preferences {
            music_gain: f32::NAN,
            ..Default::default()
        };
        assert!(preferences.validate().is_err());
        preferences.music_gain = 1.0;
        preferences.microphone_gain = 2.1;
        assert!(preferences.validate().is_err());
        let saved = serde_json::to_value(Preferences::default()).unwrap();
        assert!(saved.get("microphone_enabled").is_none());
        assert!(
            !AudioState::new(Preferences::default())
                .snapshot()
                .microphone_enabled
        );
    }
    #[test]
    fn stale_volume_updates_cannot_reopen_a_disconnected_or_muted_microphone() {
        let off = Routing {
            preferences: Preferences::default(),
            microphone_enabled: false,
            microphone_muted: false,
            confirm_microphone: false,
        };
        let mut stale = Routing {
            microphone_enabled: true,
            ..off.clone()
        };
        assert!(needs_microphone_confirmation(&off, &stale));
        stale.confirm_microphone = true;
        assert!(!needs_microphone_confirmation(&off, &stale));
        let muted = Routing {
            microphone_muted: true,
            ..stale.clone()
        };
        stale.confirm_microphone = false;
        assert!(needs_microphone_confirmation(&muted, &stale));
    }
}
