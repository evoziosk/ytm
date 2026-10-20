use serde::{Deserialize, Serialize};
use ts_rs::TS;

mod control;
pub use control::*;

pub const VERSION: u32 = 1;

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS, PartialEq)]
pub struct TrackInfo {
    pub video_id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub artwork: String,
    pub playlist_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS, PartialEq)]
pub struct PlaybackState {
    pub track: Option<TrackInfo>,
    pub playing: bool,
    pub position: f64,
    pub duration: f64,
    pub volume: f64,
    #[serde(default)]
    pub muted: bool,
    pub playback_rate: f64,
    pub buffering: bool,
    pub ad: bool,
    pub ts: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
pub struct AudioStatus {
    pub requested_high: bool,
    pub premium: bool,
    pub stream_quality: String,
    pub codec: String,
    pub bitrate: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
pub struct AccountStatus {
    pub signed_in: bool,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PlayerCommand {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    Seek { position: f64 },
    Volume { volume: f64 },
    Mute { muted: bool },
    Rate { rate: f64 },
    LoadTrack { video_id: String },
    Like,
    Dislike,
    SetLocked { locked: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PlayerEvent {
    Snapshot { state: PlaybackState },
    Seeked { state: PlaybackState },
    Ended { state: PlaybackState },
    Audio { status: AudioStatus },
    Account { status: AccountStatus },
    CommandReply { id: String, error: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum CatalogKind {
    PublicSearch,
    PublicBrowse,
    Home,
    Search,
    Library,
    Browse,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct CatalogRequest {
    pub kind: CatalogKind,
    pub query: String,
    pub browse_id: String,
    #[serde(default)]
    pub continuation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct MusicItem {
    pub key: String,
    pub title: String,
    pub subtitle: String,
    #[serde(default)]
    pub artist: String,
    pub artwork: String,
    pub kind: String,
    pub video_id: String,
    pub browse_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct CatalogPage {
    pub title: String,
    pub items: Vec<MusicItem>,
    pub signed_in: bool,
    pub message: String,
    #[serde(default)]
    pub continuation: String,
    #[serde(default)]
    pub playlist_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct CatalogReply {
    pub id: String,
    pub page: Option<CatalogPage>,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Host,
    Member,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SyncStatus {
    #[default]
    Synced,
    Buffering,
    Ad,
    Behind,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct Member {
    pub id: String,
    pub name: String,
    pub role: Role,
    pub state: SyncStatus,
    pub drift_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct SyncFrame {
    pub video_id: String,
    pub position: f64,
    pub playing: bool,
    pub host_ts: f64,
    pub seq: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PartyMsg {
    Hello {
        name: String,
        version: u32,
        token: String,
        hosting: bool,
    },
    Welcome {
        id: String,
        role: Role,
        members: Vec<Member>,
        last_sync: Option<SyncFrame>,
    },
    Ping {
        t0: f64,
    },
    Pong {
        t0: f64,
        server_t: f64,
    },
    Sync {
        frame: SyncFrame,
    },
    Status {
        state: SyncStatus,
        drift_ms: f64,
    },
    Members {
        members: Vec<Member>,
    },
    HostTransfer {
        member_id: String,
    },
    HostAway,
    Error {
        message: String,
    },
}

pub fn valid_video_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub fn valid_room(code: &str) -> bool {
    (6..=8).contains(&code.len())
        && code
            .bytes()
            .all(|b| b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789".contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ids_are_strict() {
        assert!(valid_video_id("dQw4w9WgXcQ"));
        assert!(!valid_video_id("javascript:"));
        assert!(!valid_room("ABC/DE"));
        assert!(valid_room("ABCD23"));
    }
    #[test]
    fn wire_format_is_tagged() {
        assert_eq!(
            serde_json::to_string(&PlayerCommand::Seek { position: 3.5 }).unwrap(),
            r#"{"type":"seek","position":3.5}"#
        );
    }
}
