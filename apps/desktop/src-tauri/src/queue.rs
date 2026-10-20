use crate::state::{Core, Snapshot};
use serde::Deserialize;
use tauri::{AppHandle, State};
use ytm_protocol::{PlayerCommand, Role};

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum QueueAction {
    Select { index: usize },
    Remove { index: usize },
    Move { from: usize, to: usize },
    Clear,
    Shuffle { enabled: bool },
    Repeat { mode: String },
}

pub fn next_index(
    len: usize,
    current: usize,
    forward: bool,
    ended: bool,
    repeat: &str,
) -> Option<usize> {
    if len == 0 {
        return None;
    }
    if ended && repeat == "one" {
        return Some(current.min(len - 1));
    }
    if !forward {
        return Some(if current == 0 && repeat == "all" {
            len - 1
        } else {
            current.saturating_sub(1)
        });
    }
    if current + 1 < len {
        Some(current + 1)
    } else if repeat == "all" {
        Some(0)
    } else {
        None
    }
}

pub(crate) fn edit(data: &mut Snapshot, action: &QueueAction) -> Result<(), String> {
    match *action {
        QueueAction::Remove { index } => {
            if index >= data.queue.len() {
                return Err("Track is no longer in the queue".into());
            }
            if index == data.queue_index {
                return Err("Choose another track before removing the current song".into());
            }
            data.queue.remove(index);
            if index < data.queue_index {
                data.queue_index -= 1;
            }
        }
        QueueAction::Move { from, to } => {
            if from >= data.queue.len() || to >= data.queue.len() {
                return Err("Invalid queue position".into());
            }
            let current = data.queue[data.queue_index].key.clone();
            let item = data.queue.remove(from);
            data.queue.insert(to, item);
            data.queue_index = data
                .queue
                .iter()
                .position(|item| item.key == current)
                .unwrap_or(0);
        }
        QueueAction::Clear => {
            // Keep the playing item and its privacy context; clear everything else.
            data.queue = data
                .queue
                .get(data.queue_index)
                .cloned()
                .into_iter()
                .collect();
            data.queue_index = 0;
        }
        QueueAction::Shuffle { enabled } => {
            if enabled && !data.shuffle {
                use rand::seq::SliceRandom;
                let start = (data.queue_index + 1).min(data.queue.len());
                data.queue[start..].shuffle(&mut rand::rng());
            }
            data.shuffle = enabled;
        }
        QueueAction::Repeat { ref mode } => {
            if !matches!(mode.as_str(), "off" | "all" | "one") {
                return Err("Invalid repeat mode".into());
            }
            data.repeat.clone_from(mode);
        }
        QueueAction::Select { index } => {
            if index >= data.queue.len() {
                return Err("Track is no longer in the queue".into());
            }
            data.queue_index = index;
        }
    }
    data.queue_revision = data.queue_revision.wrapping_add(1);
    Ok(())
}

#[tauri::command]
pub async fn queue_action(
    app: AppHandle,
    core: State<'_, Core>,
    action: QueueAction,
) -> Result<(), String> {
    let _guard = core.mutation.lock().await;
    if crate::control_client::active(&core) {
        return Err("Use the host queue in Remote control".into());
    }
    let command = {
        let mut data = core.data.write().expect("state lock");
        if data.party.role == Some(Role::Member) {
            return Err("Leave the party to edit your queue".into());
        }
        edit(&mut data, &action)?;
        if matches!(action, QueueAction::Select { .. }) {
            Some(PlayerCommand::LoadTrack {
                video_id: data.queue[data.queue_index].video_id.clone(),
            })
        } else {
            None
        }
    };
    core.emit(&app);
    if let Some(command) = command {
        crate::playback::confirmed(&app, command).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeat_only_repeats_one_on_natural_end() {
        assert_eq!(next_index(3, 1, true, true, "one"), Some(1));
        assert_eq!(next_index(3, 1, true, false, "one"), Some(2));
        assert_eq!(next_index(3, 2, true, true, "off"), None);
        assert_eq!(next_index(3, 2, true, true, "all"), Some(0));
        assert_eq!(next_index(0, 0, true, true, "all"), None);
    }
    #[test]
    fn editing_preserves_current_song_and_playlist_privacy() {
        let mut data = Core::new(Default::default()).snapshot();
        data.queue = (0..4)
            .map(|i| ytm_protocol::MusicItem {
                key: i.to_string(),
                title: String::new(),
                subtitle: String::new(),
                artist: String::new(),
                artwork: String::new(),
                kind: "song".into(),
                video_id: "dQw4w9WgXcQ".into(),
                browse_id: String::new(),
            })
            .collect();
        data.queue_index = 2;
        data.queue_playlist_id = "PRIVATE".into();
        edit(&mut data, &QueueAction::Move { from: 2, to: 0 }).unwrap();
        assert_eq!(data.queue[data.queue_index].key, "2");
        edit(&mut data, &QueueAction::Remove { index: 2 }).unwrap();
        assert_eq!(data.queue[data.queue_index].key, "2");
        let index = data.queue_index;
        assert!(edit(&mut data, &QueueAction::Remove { index }).is_err());
        edit(&mut data, &QueueAction::Clear).unwrap();
        assert_eq!(data.queue.len(), 1);
        assert_eq!(data.queue_playlist_id, "PRIVATE");
    }
}
