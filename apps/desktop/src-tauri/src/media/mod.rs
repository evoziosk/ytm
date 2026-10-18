use crate::state::Core;
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
};
use std::{sync::Mutex, time::Duration};
use tauri::{AppHandle, Manager};
use ytm_protocol::PlayerCommand;

#[cfg(target_os = "windows")]
pub mod audio_identity;

struct NativeMedia(Mutex<MediaControls>);
pub fn setup(app: &AppHandle, core: &Core) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "windows")]
    if let Err(error) = audio_identity::start(app) {
        tracing::warn!(%error, "Could not set Windows volume mixer identity");
    }
    #[cfg(target_os = "windows")]
    let hwnd = Some(
        app.get_webview_window("main")
            .ok_or("Main window missing")?
            .hwnd()?
            .0,
    );
    #[cfg(not(target_os = "windows"))]
    let hwnd = None;
    let mut controls = MediaControls::new(PlatformConfig {
        dbus_name: "io.ytmlisten.desktop",
        display_name: "YTM Listen",
        hwnd,
    })?;
    let handle = app.clone();
    controls.attach(move |event| {
        let command = match event {
            MediaControlEvent::Play => PlayerCommand::Play,
            MediaControlEvent::Pause => PlayerCommand::Pause,
            MediaControlEvent::Toggle => PlayerCommand::Toggle,
            MediaControlEvent::Next => PlayerCommand::Next,
            MediaControlEvent::Previous => PlayerCommand::Previous,
            MediaControlEvent::SetPosition(position) => PlayerCommand::Seek {
                position: position.0.as_secs_f64(),
            },
            MediaControlEvent::SetVolume(volume) => PlayerCommand::Volume { volume },
            _ => return,
        };
        let _ = crate::commands::user_command(&handle, &command);
    })?;
    app.manage(NativeMedia(Mutex::new(controls)));
    let mut receiver = core.changes.subscribe();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let player = match receiver.recv().await {
                Ok(p) => p,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            };
            let app = handle.clone();
            let _ = handle.run_on_main_thread(move || {
                let media = app.state::<NativeMedia>();
                let Ok(mut controls) = media.0.lock() else {
                    return;
                };
                if let Some(track) = player.track {
                    let _ = controls.set_metadata(MediaMetadata {
                        title: Some(&track.title),
                        artist: Some(&track.artist),
                        album: Some(&track.album),
                        cover_url: if track.artwork.is_empty() {
                            None
                        } else {
                            Some(&track.artwork)
                        },
                        duration: Some(Duration::from_secs_f64(player.duration)),
                    });
                }
                let progress = Some(MediaPosition(Duration::from_secs_f64(player.position)));
                let _ = controls.set_playback(if player.playing {
                    MediaPlayback::Playing { progress }
                } else {
                    MediaPlayback::Paused { progress }
                });
            });
        }
    });
    Ok(())
}
