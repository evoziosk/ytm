//! Platform-independent admission, authorization and retry policy. Playback remains on the host.
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use ytm_protocol::*;

pub fn secret() -> String {
    (0..32)
        .map(|_| format!("{:02x}", rand::rng().random::<u8>()))
        .collect()
}
pub fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
pub fn valid_secret(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairedDevice {
    pub id: String,
    pub name: String,
    pub credential_hash: String,
    pub permissions: ControlPermissions,
}

#[derive(Clone)]
pub struct Session {
    pub id: String,
    pub invitation: String,
    pub host_name: String,
    pub mode: ControlMode,
    pub participants: HashMap<String, ControlParticipant>,
    pub paired: Vec<PairedDevice>,
    credentials: HashMap<String, String>,
    revoked: HashSet<String>,
    replies: HashMap<(String, String), (String, ControlResult)>,
}
impl Session {
    pub fn new(host_name: String, mode: ControlMode, paired: Vec<PairedDevice>) -> Self {
        Self {
            id: secret(),
            invitation: secret(),
            host_name,
            mode,
            participants: HashMap::new(),
            paired,
            credentials: HashMap::new(),
            revoked: HashSet::new(),
            replies: HashMap::new(),
        }
    }
    pub fn connect(
        &mut self,
        version: u32,
        session_id: &str,
        invitation: &str,
        credential: &str,
        name: &str,
        mode: ControlMode,
    ) -> Result<ControlParticipant, String> {
        if version != CONTROL_VERSION
            || session_id != self.id
            || mode != self.mode
            || !valid_secret(credential)
            || name.trim().is_empty()
            || name.len() > 64
        {
            return Err("Invalid session or device identity".into());
        }
        let hash = digest(credential);
        if self.revoked.contains(&hash) {
            return Err("Device removed; request a new invitation".into());
        }
        if let Some(id) = self.credentials.get(&hash) {
            let peer = self.participants.get_mut(id).ok_or("Device removed")?;
            if peer.connected {
                return Err("Device is already connected".into());
            }
            peer.connected = true;
            return Ok(peer.clone());
        }
        let paired = self
            .paired
            .iter()
            .find(|device| device.credential_hash == hash && mode == ControlMode::Personal)
            .cloned();
        if paired.is_none() && invitation != self.invitation {
            return Err("Invitation expired".into());
        }
        if self.participants.len() >= CONTROL_MAX_CONTROLLERS {
            return Err("Session is full (20 controllers)".into());
        }
        let peer = ControlParticipant {
            id: paired
                .as_ref()
                .map(|device| device.id.clone())
                .unwrap_or_else(secret),
            name: paired
                .as_ref()
                .map(|device| device.name.clone())
                .unwrap_or_else(|| name.trim().into()),
            mode,
            role: if mode == ControlMode::Personal {
                ControlRole::PersonalRemote
            } else {
                ControlRole::GroupGuest
            },
            approved: paired.is_some(),
            connected: true,
            permissions: paired.map(|device| device.permissions).unwrap_or_default(),
        };
        self.credentials.insert(hash, peer.id.clone());
        self.participants.insert(peer.id.clone(), peer.clone());
        Ok(peer)
    }
    pub fn approve(&mut self, id: &str, permissions: ControlPermissions) -> Result<(), String> {
        let peer = self
            .participants
            .get_mut(id)
            .ok_or("Participant not found")?;
        peer.approved = true;
        peer.permissions = permissions.clone();
        if peer.mode == ControlMode::Personal {
            let hash = self
                .credentials
                .iter()
                .find(|(_, value)| value.as_str() == id)
                .map(|(hash, _)| hash.clone())
                .ok_or("Missing credential")?;
            self.paired.retain(|device| device.id != id);
            self.paired.push(PairedDevice {
                id: id.into(),
                name: peer.name.clone(),
                credential_hash: hash,
                permissions,
            });
        }
        Ok(())
    }
    pub fn remove(&mut self, id: &str) {
        self.participants.remove(id);
        self.paired.retain(|device| device.id != id);
        for (hash, _) in self
            .credentials
            .iter()
            .filter(|(_, value)| value.as_str() == id)
        {
            self.revoked.insert(hash.clone());
        }
        self.credentials.retain(|_, value| value != id);
    }
    pub fn disconnect(&mut self, id: &str) {
        if let Some(peer) = self.participants.get_mut(id) {
            peer.connected = false;
        }
    }
    pub fn rotate(&mut self) {
        self.invitation = secret();
    }
    pub fn authorize(&self, id: &str, command: &ControlCommand) -> Result<(), String> {
        let peer = self
            .participants
            .get(id)
            .filter(|p| p.approved && p.connected)
            .ok_or("Host approval required")?;
        let p = &peer.permissions;
        let allowed = match command {
            ControlCommand::Snapshot | ControlCommand::Queue { .. } => true,
            ControlCommand::Search { .. } | ControlCommand::Browse { .. } => p.catalog,
            ControlCommand::Playback { command } => match command {
                RemotePlayback::Play | RemotePlayback::Pause => p.transport,
                RemotePlayback::Next | RemotePlayback::Previous => p.skip,
                RemotePlayback::Seek { .. } => p.seek,
                RemotePlayback::Volume { .. } => p.volume,
            },
            ControlCommand::EditQueue { edit, .. } => {
                p.queue
                    && match edit {
                        RemoteQueueEdit::Clear
                        | RemoteQueueEdit::Insert {
                            placement: QueuePlacement::Replace,
                            ..
                        } => p.replace_queue,
                        RemoteQueueEdit::Select { .. }
                        | RemoteQueueEdit::Insert {
                            placement: QueuePlacement::Now,
                            ..
                        } => p.transport,
                        _ => true,
                    }
            }
        };
        if allowed {
            Ok(())
        } else {
            Err("Host has not granted this permission".into())
        }
    }
    /// Cache is session-bounded. Never evict mutation IDs and risk applying an old retry twice.
    pub fn previous(
        &self,
        participant: &str,
        request: &ControlRequest,
    ) -> Result<Option<ControlResult>, String> {
        if request.id.len() != 32 || !request.id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Invalid request ID".into());
        }
        if let Some((command, result)) = self.replies.get(&(participant.into(), request.id.clone()))
        {
            if command != &serde_json::to_string(&request.command).map_err(|e| e.to_string())? {
                return Err("Request ID reused for another command".into());
            }
            return Ok(Some(result.clone()));
        }
        if matches!(
            request.command,
            ControlCommand::Playback { .. } | ControlCommand::EditQueue { .. }
        ) && self
            .replies
            .keys()
            .filter(|(id, _)| id == participant)
            .count()
            >= 512
        {
            return Err("Command history full; start a new sharing session".into());
        }
        Ok(None)
    }
    pub fn remember(&mut self, participant: &str, request: &ControlRequest, result: ControlResult) {
        if matches!(
            request.command,
            ControlCommand::Playback { .. } | ControlCommand::EditQueue { .. }
        ) {
            self.replies.insert(
                (participant.into(), request.id.clone()),
                (
                    serde_json::to_string(&request.command).expect("command JSON"),
                    result,
                ),
            );
        }
    }
}

pub fn validate_track(track: &RemoteTrack) -> bool {
    valid_video_id(&track.video_id)
        && track.title.len() <= 2048
        && track.artist.len() <= 2048
        && track.artwork.len() <= 2048
        && (track.artwork.is_empty() || track.artwork.starts_with("https://"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn join(s: &mut Session) -> (ControlParticipant, String) {
        let credential = secret();
        let p = s
            .connect(
                1,
                &s.id.clone(),
                &s.invitation.clone(),
                &credential,
                "Guest",
                s.mode,
            )
            .unwrap();
        (p, credential)
    }
    #[test]
    fn admission_permissions_revocation_and_rotation() {
        let mut s = Session::new("Host".into(), ControlMode::Group, vec![]);
        let (p, token) = join(&mut s);
        assert!(s.authorize(&p.id, &ControlCommand::Snapshot).is_err());
        s.approve(&p.id, Default::default()).unwrap();
        assert!(s
            .authorize(
                &p.id,
                &ControlCommand::Playback {
                    command: RemotePlayback::Volume { volume: 0.5 }
                }
            )
            .is_err());
        assert!(s
            .authorize(
                &p.id,
                &ControlCommand::EditQueue {
                    revision: 0,
                    edit: RemoteQueueEdit::Clear
                }
            )
            .is_err());
        let invite = s.invitation.clone();
        s.rotate();
        assert!(s
            .connect(1, &s.id.clone(), &invite, &secret(), "new", s.mode)
            .is_err());
        s.remove(&p.id);
        assert!(s
            .connect(
                1,
                &s.id.clone(),
                &s.invitation.clone(),
                &token,
                "Guest",
                s.mode
            )
            .is_err());
    }
    #[test]
    fn personal_pairing_persists_but_group_admission_does_not() {
        for mode in [ControlMode::Personal, ControlMode::Group] {
            let mut s = Session::new("Host".into(), mode, vec![]);
            let (p, token) = join(&mut s);
            s.approve(&p.id, Default::default()).unwrap();
            let mut next = Session::new("Host".into(), mode, s.paired);
            assert_eq!(
                next.connect(1, &next.id.clone(), "", &token, "Guest", mode)
                    .is_ok(),
                mode == ControlMode::Personal
            );
        }
    }
    #[test]
    fn selecting_a_queue_entry_requires_transport_and_queue_permissions() {
        let mut session = Session::new("Host".into(), ControlMode::Personal, vec![]);
        let (peer, _) = join(&mut session);
        let command = ControlCommand::EditQueue {
            revision: 4,
            edit: RemoteQueueEdit::Select {
                entry_id: "entry".into(),
            },
        };
        session
            .approve(
                &peer.id,
                ControlPermissions {
                    transport: false,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(session.authorize(&peer.id, &command).is_err());
        session
            .approve(
                &peer.id,
                ControlPermissions {
                    queue: false,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(session.authorize(&peer.id, &command).is_err());
        session.approve(&peer.id, Default::default()).unwrap();
        assert!(session.authorize(&peer.id, &command).is_ok());
    }
    #[test]
    fn retry_identity_is_bound_to_device_and_command() {
        let mut s = Session::new("Host".into(), ControlMode::Group, vec![]);
        let r = ControlRequest {
            id: "a".repeat(32),
            command: ControlCommand::Playback {
                command: RemotePlayback::Next,
            },
        };
        assert!(s.previous("one", &r).unwrap().is_none());
        s.remember("one", &r, ControlResult::Ok);
        assert!(s.previous("one", &r).unwrap().is_some());
        assert!(s.previous("two", &r).unwrap().is_none());
        assert!(s
            .previous(
                "one",
                &ControlRequest {
                    id: r.id,
                    command: ControlCommand::Snapshot
                }
            )
            .is_err());
    }
    #[test]
    fn admission_limit_includes_pending_connections() {
        let mut s = Session::new("Host".into(), ControlMode::Group, vec![]);
        for _ in 0..20 {
            join(&mut s);
        }
        assert!(s
            .connect(
                1,
                &s.id.clone(),
                &s.invitation.clone(),
                &secret(),
                "21",
                s.mode
            )
            .is_err());
    }
    #[test]
    fn wire_rejects_private_or_unknown_operations() {
        assert!(serde_json::from_str::<ControlClientMsg>(
            r#"{"type":"request","request":{"id":"abc","command":{"type":"library"}}}"#
        )
        .is_err());
        assert!(serde_json::from_str::<RemoteTrack>(r#"{"video_id":"dQw4w9WgXcQ","title":"x","artist":"","artwork":"","playlist_id":"PRIVATE"}"#).is_err());
    }
}
