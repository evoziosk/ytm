use crate::{
    settings::Settings,
    state::{Core, Snapshot},
};
use tauri::{AppHandle, Manager, State, WebviewWindow};
use ytm_protocol::{valid_video_id, PlayerCommand, PlayerEvent, Role};

#[tauri::command]
pub fn player_event(
    app: AppHandle,
    window: WebviewWindow,
    core: State<Core>,
    event: PlayerEvent,
) -> Result<(), String> {
    if window.label() != "main"
        || window
            .url()
            .map_err(|e| e.to_string())?
            .origin()
            .ascii_serialization()
            != "https://music.youtube.com"
    {
        return Err("Invalid player origin".into());
    }
    accept_event(&app, &core, event)
}
pub(crate) fn accept_event(app: &AppHandle, core: &Core, event: PlayerEvent) -> Result<(), String> {
    if let PlayerEvent::CommandReply { id, error } = &event {
        if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) || error.len() > 2048 {
            return Err("Invalid playback reply".into());
        }
        if let Some(sender) = core
            .command_pending
            .lock()
            .expect("command lock")
            .remove(id)
        {
            let _ = sender.send(if error.is_empty() {
                Ok(())
            } else {
                Err(error.clone())
            });
        }
        return Ok(());
    }
    if let PlayerEvent::Account { status } = &event {
        if status.name.len() > 512 {
            return Err("Invalid account name".into());
        }
        {
            let mut data = core.data.write().expect("state lock");
            if data.account.signed_in && !status.signed_in {
                data.queue.clear();
                data.queue_revision = data.queue_revision.wrapping_add(1);
                data.queue_index = 0;
                data.queue_playlist_id.clear();
                data.previous_playlist = None;
            }
            data.account = status.clone();
            data.account_message = if status.signed_in {
                if status.name.is_empty() {
                    "Signed in to YouTube Music".into()
                } else {
                    format!("Signed in as {}", status.name)
                }
            } else {
                "Listening without an account".into()
            };
        }
        core.emit(app);
        return Ok(());
    }
    if let PlayerEvent::Audio { status } = &event {
        if status.codec.len() > 128
            || status.bitrate > 10000000
            || !matches!(
                status.stream_quality.as_str(),
                "high" | "normal" | "low" | "unknown"
            )
        {
            return Err("Invalid audio status".into());
        }
        core.data.write().expect("state lock").audio = status.clone();
        core.emit(app);
        return Ok(());
    }
    let ended = matches!(&event, PlayerEvent::Ended { .. }) && !core.snapshot().player.ad;
    let state = match event {
        PlayerEvent::Snapshot { state }
        | PlayerEvent::Seeked { state }
        | PlayerEvent::Ended { state } => state,
        PlayerEvent::Audio { .. } => unreachable!("audio status handled above"),
        PlayerEvent::Account { .. } => unreachable!("account status handled above"),
        PlayerEvent::CommandReply { .. } => unreachable!("command reply handled above"),
    };
    if ![
        state.position,
        state.duration,
        state.volume,
        state.ts,
        state.playback_rate,
    ]
    .iter()
    .all(|v| v.is_finite())
        || !(0.0..=604800.0).contains(&state.position)
        || !(0.0..=604800.0).contains(&state.duration)
        || !(0.0..=1.0).contains(&state.volume)
        || !(0.0..=16.0).contains(&state.playback_rate)
    {
        return Err("Invalid playback values".into());
    }
    if let Some(track) = &state.track {
        if !valid_video_id(&track.video_id)
            || track.title.len() > 2048
            || track.artist.len() > 2048
            || track.album.len() > 2048
            || track.playlist_id.len() > 512
            || track.artwork.len() > 2048
        {
            return Err("Invalid track metadata".into());
        }
        if !track.artwork.is_empty()
            && url::Url::parse(&track.artwork)
                .map(|u| u.scheme() != "https")
                .unwrap_or(true)
        {
            return Err("Artwork must use HTTPS".into());
        }
    }
    let music_ended = ended && !state.ad;
    let controlling_remote = crate::control_client::active(core);
    let silence_controller = controlling_remote && state.playing;
    core.update_player(app, state);
    if silence_controller {
        let _ = dispatch(app, &PlayerCommand::Pause);
    }
    if music_ended && !controlling_remote {
        let handle = app.clone();
        tauri::async_runtime::spawn(async move {
            let core = handle.state::<Core>();
            let _guard = core.mutation.lock().await;
            let _ = crate::catalog::advance(&handle, true, true).await;
        });
    }
    Ok(())
}
#[tauri::command]
pub fn get_snapshot(core: State<Core>) -> Snapshot {
    core.snapshot()
}

pub fn dispatch(app: &AppHandle, command: &PlayerCommand) -> Result<(), String> {
    let payload = serde_json::to_string(command).map_err(|e| e.to_string())?;
    crate::backend::evaluate(app, &format!("window.__YTM_LISTEN_COMMAND__?.({payload})"))
}
pub fn user_command(app: &AppHandle, command: &PlayerCommand) -> Result<(), String> {
    let handle = app.clone();
    let command = command.clone();
    tauri::async_runtime::spawn(async move {
        let _ = perform_user_command(&handle, command).await;
    });
    Ok(())
}
pub async fn perform_user_command(app: &AppHandle, command: PlayerCommand) -> Result<(), String> {
    let core = app.state::<Core>();
    let _guard = core.mutation.lock().await;
    perform_serialized(app, command).await
}
pub(crate) async fn perform_serialized(
    app: &AppHandle,
    command: PlayerCommand,
) -> Result<(), String> {
    if crate::control_client::active(&app.state::<Core>()) {
        return crate::control_client::playback(&app.state::<Core>(), command).await;
    }
    if app.state::<Core>().snapshot().party.role == Some(Role::Member)
        && !matches!(
            &command,
            PlayerCommand::Volume { .. }
                | PlayerCommand::Mute { .. }
                | PlayerCommand::Like
                | PlayerCommand::Dislike
        )
    {
        return Err("Playback follows the host. Leave the party to control your music.".into());
    }
    match &command {
        PlayerCommand::Next if crate::catalog::advance(app, true, false).await? => return Ok(()),
        PlayerCommand::Previous if crate::catalog::advance(app, false, false).await? => {
            return Ok(())
        }
        PlayerCommand::Seek { position }
            if !position.is_finite() || !(0.0..=604800.0).contains(position) =>
        {
            return Err("Invalid seek position".into())
        }
        PlayerCommand::Volume { volume }
            if !volume.is_finite() || !(0.0..=1.0).contains(volume) =>
        {
            return Err("Invalid volume".into())
        }
        PlayerCommand::LoadTrack { video_id } if !valid_video_id(video_id) => {
            return Err("Invalid track ID".into())
        }
        PlayerCommand::SetLocked { .. } | PlayerCommand::Rate { .. } => {
            return Err("Internal command".into())
        }
        _ => (),
    }
    crate::playback::confirmed(app, command).await
}
#[tauri::command]
pub async fn send_command(app: AppHandle, command: PlayerCommand) -> Result<(), String> {
    perform_user_command(&app, command).await
}
#[tauri::command]
pub async fn save_settings(
    app: AppHandle,
    core: State<'_, Core>,
    settings: Settings,
) -> Result<(), String> {
    commit_settings(&app, &core, |_| settings)
}

pub(crate) fn commit_settings(
    app: &AppHandle,
    core: &Core,
    update: impl FnOnce(Settings) -> Settings,
) -> Result<(), String> {
    let _guard = core.settings_update.lock().expect("settings lock");
    let current = core.data.read().expect("state lock").settings.clone();
    let mut settings = update(current);
    settings.validate()?;
    let previous_blur = core.data.read().expect("state lock").settings.window_blur;
    let changed_blur = previous_blur != settings.window_blur;
    if changed_blur {
        crate::windows::set_app_blur(app, settings.window_blur)?;
    }
    if let Err(error) = crate::settings::save(app, &settings) {
        if changed_blur {
            let _ = crate::windows::set_app_blur(app, previous_blur);
        }
        return Err(error);
    }
    core.data.write().expect("state lock").settings = settings;
    core.emit(app);
    Ok(())
}

#[tauri::command]
pub async fn set_window_blur(
    app: AppHandle,
    window: WebviewWindow,
    core: State<'_, Core>,
    enabled: bool,
) -> Result<(), String> {
    if window.label() != "settings" {
        return Err("Change window appearance from the main app".into());
    }
    commit_settings(&app, &core, |mut settings| {
        settings.window_blur = enabled;
        settings
    })
}
#[tauri::command]
pub async fn open_panel(app: AppHandle, panel: String) -> Result<(), String> {
    crate::windows::open_panel(&app, &panel)
}
#[tauri::command]
pub fn show_music(app: AppHandle) -> Result<(), String> {
    crate::windows::show_music(&app)
}
#[tauri::command]
pub async fn open_account(app: AppHandle) -> Result<(), String> {
    crate::windows::open_account(&app)
}
#[tauri::command]
pub async fn logout_account(app: AppHandle) -> Result<(), String> {
    crate::windows::logout_account(&app)
}
#[tauri::command]
pub fn refresh_music(app: AppHandle) -> Result<(), String> {
    crate::backend::refresh(&app)
}
#[tauri::command]
pub async fn party_connect(
    app: AppHandle,
    core: State<'_, Core>,
    code: String,
    hosting: bool,
) -> Result<(), String> {
    let _guard = core.mutation.lock().await;
    crate::party::client::start(&app, &core, &code, hosting)
}
#[tauri::command]
pub fn party_leave(app: AppHandle, core: State<Core>) {
    crate::party::client::leave(&app, &core);
}
#[tauri::command]
pub async fn party_transfer(core: State<'_, Core>, member_id: String) -> Result<(), String> {
    if core.snapshot().party.role != Some(Role::Host) {
        return Err("Only the host can transfer the room".into());
    }
    let sender = core
        .party_out
        .lock()
        .expect("party sender")
        .clone()
        .ok_or("Not connected")?;
    sender
        .send(ytm_protocol::PartyMsg::HostTransfer { member_id })
        .await
        .map_err(|e| e.to_string())
}
