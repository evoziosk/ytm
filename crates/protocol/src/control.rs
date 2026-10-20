use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const CONTROL_VERSION: u32 = 1;
pub const CONTROL_MAX_MESSAGE: usize = 65536;
pub const CONTROL_PAGE_SIZE: usize = 50;
pub const CONTROL_MAX_CONTROLLERS: usize = 20;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ControlMode {
    Personal,
    Group,
}

/// Public LAN connection metadata. Deliberately excludes invitation and device secrets.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ControlDiscovery {
    pub version: u32,
    pub session_id: String,
    pub host_name: String,
    pub mode: ControlMode,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ControlRole {
    Host,
    PersonalRemote,
    GroupGuest,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ControlPermissions {
    pub transport: bool,
    pub skip: bool,
    pub seek: bool,
    pub queue: bool,
    pub replace_queue: bool,
    pub volume: bool,
    pub catalog: bool,
}
impl Default for ControlPermissions {
    fn default() -> Self {
        Self {
            transport: true,
            skip: true,
            seek: true,
            queue: true,
            replace_queue: false,
            volume: false,
            catalog: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ControlParticipant {
    pub id: String,
    pub name: String,
    pub mode: ControlMode,
    pub role: ControlRole,
    pub approved: bool,
    pub connected: bool,
    pub permissions: ControlPermissions,
}

/// Explicit public metadata. Collection identifiers and account fields never cross this boundary.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct RemoteTrack {
    pub video_id: String,
    pub title: String,
    pub artist: String,
    pub artwork: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct RemoteQueueEntry {
    pub id: String,
    pub track: RemoteTrack,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct RemoteCollection {
    pub browse_id: String,
    pub title: String,
    pub kind: String,
    pub artwork: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct RemoteSnapshot {
    pub session_id: String,
    pub host_name: String,
    pub track: Option<RemoteTrack>,
    pub playing: bool,
    pub position: f64,
    pub duration: f64,
    pub volume: f64,
    pub muted: bool,
    pub buffering: bool,
    pub queue_revision: u32,
    pub current_entry: Option<String>,
    pub participants: Vec<ControlParticipant>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RemotePlayback {
    Play,
    Pause,
    Next,
    Previous,
    Seek { position: f64 },
    Volume { volume: f64 },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum QueuePlacement {
    Now,
    Next,
    End,
    Replace,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RemoteQueueEdit {
    Select {
        entry_id: String,
    },
    Insert {
        tracks: Vec<RemoteTrack>,
        placement: QueuePlacement,
    },
    Remove {
        entry_id: String,
    },
    Move {
        entry_id: String,
        before_id: String,
    },
    Clear,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlCommand {
    Snapshot,
    Queue {
        offset: u32,
        limit: u32,
    },
    Playback {
        command: RemotePlayback,
    },
    EditQueue {
        revision: u32,
        edit: RemoteQueueEdit,
    },
    Search {
        query: String,
        offset: u32,
        limit: u32,
    },
    Browse {
        browse_id: String,
        offset: u32,
        limit: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ControlRequest {
    pub id: String,
    pub command: ControlCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlClientMsg {
    Hello {
        version: u32,
        session_id: String,
        invitation: String,
        credential: String,
        name: String,
        mode: ControlMode,
    },
    Request {
        request: ControlRequest,
    },
    Disconnect,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlResult {
    Ok,
    Snapshot {
        snapshot: RemoteSnapshot,
    },
    Queue {
        revision: u32,
        total: u32,
        offset: u32,
        entries: Vec<RemoteQueueEntry>,
    },
    Catalog {
        total: u32,
        offset: u32,
        tracks: Vec<RemoteTrack>,
        collections: Vec<RemoteCollection>,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlServerMsg {
    Admission { participant: ControlParticipant },
    Reply { id: String, result: ControlResult },
    State { snapshot: RemoteSnapshot },
    HostAway,
    Ended,
    Error { message: String },
}
