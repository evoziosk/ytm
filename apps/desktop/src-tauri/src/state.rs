use crate::settings::Settings;
use serde::Serialize;
use std::sync::{Arc, RwLock};
use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast;
use ytm_protocol::{
    AccountStatus, AudioStatus, Member, MusicItem, PlaybackState, PlayerCommand, Role,
};
type CatalogPending = std::collections::HashMap<
    String,
    tokio::sync::oneshot::Sender<Result<ytm_protocol::CatalogPage, String>>,
>;
type CommandPending =
    std::collections::HashMap<String, tokio::sync::oneshot::Sender<Result<(), String>>>;

pub fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
        * 1000.0
}
#[derive(Debug, Clone, Default, Serialize)]
pub struct PartyState {
    pub code: String,
    pub role: Option<Role>,
    pub members: Vec<Member>,
    pub connected: bool,
    pub reconnecting: bool,
    pub drift_ms: f64,
    pub message: String,
    pub invite_url: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub player: PlaybackState,
    pub settings: Settings,
    pub party: PartyState,
    pub discord: String,
    pub bridge_connected: bool,
    pub pending_room: Option<String>,
    pub queue: Vec<MusicItem>,
    pub queue_index: usize,
    pub queue_revision: u32,
    pub queue_playlist_id: String,
    #[serde(skip)]
    pub previous_playlist: Option<(String, String)>,
    pub shuffle: bool,
    pub repeat: String,
    pub playback_error: String,
    pub account_message: String,
    pub audio: AudioStatus,
    pub account: AccountStatus,
}
#[derive(Clone)]
pub struct Core {
    pub remote: Arc<std::sync::Mutex<crate::control_client::ClientState>>,
    pub mutation: Arc<tokio::sync::Mutex<()>>,
    pub control: Arc<std::sync::Mutex<crate::control::ControlState>>,
    pub data: Arc<RwLock<Snapshot>>,
    pub changes: broadcast::Sender<PlaybackState>,
    pub party_stop: Arc<std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
    pub party_out: Arc<std::sync::Mutex<Option<tokio::sync::mpsc::Sender<ytm_protocol::PartyMsg>>>>,
    pub party_generation: Arc<std::sync::atomic::AtomicU64>,
    pub catalog_pending: Arc<std::sync::Mutex<CatalogPending>>,
    pub command_pending: Arc<std::sync::Mutex<CommandPending>>,
    pub failed_command: Arc<std::sync::Mutex<Option<PlayerCommand>>>,
    pub settings_update: Arc<std::sync::Mutex<()>>,
}
impl Core {
    pub fn new(settings: Settings) -> Self {
        let (changes, _) = broadcast::channel(64);
        Self {
            remote: Default::default(),
            mutation: Default::default(),
            control: Default::default(),
            data: Arc::new(RwLock::new(Snapshot {
                player: PlaybackState::default(),
                settings,
                party: PartyState::default(),
                discord: "Not configured".into(),
                bridge_connected: false,
                pending_room: None,
                queue: vec![],
                queue_index: 0,
                queue_revision: 0,
                queue_playlist_id: String::new(),
                previous_playlist: None,
                shuffle: false,
                repeat: "off".into(),
                playback_error: String::new(),
                account_message: String::new(),
                audio: AudioStatus::default(),
                account: AccountStatus::default(),
            })),
            changes,
            party_stop: Default::default(),
            party_out: Default::default(),
            party_generation: Default::default(),
            catalog_pending: Default::default(),
            command_pending: Default::default(),
            failed_command: Default::default(),
            settings_update: Default::default(),
        }
    }
    pub fn snapshot(&self) -> Snapshot {
        self.data.read().expect("state lock").clone()
    }
    pub fn emit(&self, app: &AppHandle) {
        let snapshot = self.snapshot();
        // Never broadcast private settings or room credentials to the remote page.
        for label in ["settings", "mini", "party", "lyrics", "search"] {
            let _ = app.emit_to(label, "state", &snapshot);
        }
    }
    pub fn update_player(&self, app: &AppHandle, mut player: PlaybackState) {
        player.ts = now_ms();
        {
            let mut data = self.data.write().expect("state lock");
            // Keep the source collection through Music's paired song/video navigation.
            apply_playlist(&mut player, &mut data);
            data.player = player.clone();
            data.bridge_connected = true;
        }
        let _ = self.changes.send(player.clone());
        for label in ["settings", "mini", "party", "lyrics", "search"] {
            let _ = app.emit_to(label, "player-state", &player);
        }
    }
}

fn apply_playlist(player: &mut PlaybackState, data: &mut Snapshot) {
    if let Some(track) = &mut player.track {
        if let Some((previous_id, playlist)) = &data.previous_playlist {
            if track.video_id == *previous_id || player.ad {
                track.playlist_id.clone_from(playlist);
                return;
            }
            data.previous_playlist = None;
        }
        if !data.queue.is_empty() {
            track.playlist_id.clone_from(&data.queue_playlist_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_playlist_privacy_survives_loading_and_song_counterparts() {
        let mut data = Core::new(Settings::default()).snapshot();
        data.queue.push(MusicItem {
            key: "new".into(),
            title: String::new(),
            subtitle: String::new(),
            artist: String::new(),
            artwork: String::new(),
            kind: "song".into(),
            video_id: "dQw4w9WgXcQ".into(),
            browse_id: String::new(),
        });
        data.queue_playlist_id = "NEW_PRIVATE".into();
        data.previous_playlist = Some(("old_video11".into(), "OLD_PRIVATE".into()));
        let mut player = PlaybackState {
            track: Some(ytm_protocol::TrackInfo {
                video_id: "old_video11".into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        apply_playlist(&mut player, &mut data);
        assert_eq!(player.track.as_ref().unwrap().playlist_id, "OLD_PRIVATE");
        player.track.as_mut().unwrap().video_id = "paired_song".into();
        apply_playlist(&mut player, &mut data);
        assert_eq!(player.track.as_ref().unwrap().playlist_id, "NEW_PRIVATE");
    }
}
