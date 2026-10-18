use crate::state::Core;
use tauri::{AppHandle, Manager};
use ytm_protocol::PlayerCommand;

pub async fn confirmed(app: &AppHandle, command: PlayerCommand) -> Result<(), String> {
    let core = app.state::<Core>();
    let command = if command == PlayerCommand::Toggle {
        if core.data.read().expect("state lock").player.playing {
            PlayerCommand::Pause
        } else {
            PlayerCommand::Play
        }
    } else {
        command
    };
    let result = request(app, &core, &command).await;
    if result.as_ref().is_err_and(|error| error == "Superseded") {
        return Ok(());
    }
    {
        let mut failed = core.failed_command.lock().expect("command lock");
        if let Err(error) = &result {
            *failed = Some(command);
            core.data
                .write()
                .expect("state lock")
                .playback_error
                .clone_from(error);
        } else if failed
            .as_ref()
            .is_some_and(|previous| group(previous) == group(&command))
        {
            *failed = None;
            core.data
                .write()
                .expect("state lock")
                .playback_error
                .clear();
        }
    }
    core.emit(app);
    result
}

fn group(command: &PlayerCommand) -> &'static str {
    match command {
        PlayerCommand::LoadTrack { .. } | PlayerCommand::Next | PlayerCommand::Previous => "track",
        PlayerCommand::Play | PlayerCommand::Pause | PlayerCommand::Toggle => "transport",
        PlayerCommand::Volume { .. } | PlayerCommand::Mute { .. } => "volume",
        PlayerCommand::Seek { .. } => "seek",
        PlayerCommand::Like => "like",
        PlayerCommand::Dislike => "dislike",
        _ => "internal",
    }
}

#[tauri::command]
pub fn dismiss_playback_error(app: AppHandle) {
    let core = app.state::<Core>();
    *core.failed_command.lock().expect("command lock") = None;
    core.data
        .write()
        .expect("state lock")
        .playback_error
        .clear();
    core.emit(&app);
}

async fn request(app: &AppHandle, core: &Core, command: &PlayerCommand) -> Result<(), String> {
    let main = app
        .get_webview_window("main")
        .ok_or("Playback service unavailable")?;
    if main
        .url()
        .map_err(|e| e.to_string())?
        .origin()
        .ascii_serialization()
        != "https://music.youtube.com"
    {
        return Err("Finish signing in, then retry playback.".into());
    }
    use rand::Rng;
    let id: String = (0..16)
        .map(|_| format!("{:02x}", rand::rng().random::<u8>()))
        .collect();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    {
        let mut pending = core.command_pending.lock().expect("command lock");
        if pending.len() >= 32 {
            return Err("Please wait for playback to respond.".into());
        }
        pending.insert(id.clone(), sender);
    }
    let payload = serde_json::to_string(command).map_err(|e| e.to_string())?;
    if let Err(error) = main.eval(format!(
        "window.__YTM_LISTEN_REQUEST__?.({id:?}, {payload})"
    )) {
        core.command_pending
            .lock()
            .expect("command lock")
            .remove(&id);
        return Err(error.to_string());
    }
    let result = tokio::time::timeout(std::time::Duration::from_secs(12), receiver).await;
    core.command_pending
        .lock()
        .expect("command lock")
        .remove(&id);
    result
        .map_err(|_| "Playback did not respond. Check your connection, then retry.".to_string())?
        .map_err(|_| "Playback connection changed. Please retry.".to_string())?
}

#[tauri::command]
pub async fn retry_playback(app: AppHandle) -> Result<(), String> {
    let command = app
        .state::<Core>()
        .failed_command
        .lock()
        .expect("command lock")
        .clone()
        .ok_or("Nothing to retry")?;
    crate::commands::perform_user_command(&app, command).await
}
