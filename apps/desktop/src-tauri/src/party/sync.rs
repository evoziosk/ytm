use ytm_protocol::{PlaybackState, PlayerCommand, SyncFrame, SyncStatus};

pub struct Decision {
    pub commands: Vec<PlayerCommand>,
    pub drift_ms: f64,
    pub status: SyncStatus,
}
pub fn decide(local: &PlaybackState, host: &SyncFrame, local_now: f64, offset: f64) -> Decision {
    let mut result = Decision {
        commands: vec![],
        drift_ms: 0.0,
        status: SyncStatus::Synced,
    };
    if local.ad {
        result.status = SyncStatus::Ad;
        if (local.playback_rate - 1.0).abs() > 0.001 {
            result.commands.push(PlayerCommand::Rate { rate: 1.0 });
        }
        return result;
    }
    if local.track.as_ref().map(|t| t.video_id.as_str()) != Some(host.video_id.as_str()) {
        result.commands.push(PlayerCommand::LoadTrack {
            video_id: host.video_id.clone(),
        });
        result.status = SyncStatus::Buffering;
        return result;
    }
    if local.buffering {
        result.status = SyncStatus::Buffering;
        if (local.playback_rate - 1.0).abs() > 0.001 {
            result.commands.push(PlayerCommand::Rate { rate: 1.0 });
        }
        if local.playing != host.playing {
            result.commands.push(if host.playing {
                PlayerCommand::Play
            } else {
                PlayerCommand::Pause
            });
        }
        return result;
    }
    let elapsed = ((local_now + offset - host.host_ts) / 1000.0).max(0.0);
    let mut target = host.position + if host.playing { elapsed } else { 0.0 };
    if local.duration > 0.0 {
        target = target.min(local.duration);
    }
    let local_position = local.position
        + if local.playing {
            ((local_now - local.ts) / 1000.0).max(0.0) * local.playback_rate
        } else {
            0.0
        };
    let drift = target - local_position;
    result.drift_ms = drift * 1000.0;
    if drift.abs() > 1.5 || !host.playing && drift.abs() > 0.3 {
        result
            .commands
            .push(PlayerCommand::Seek { position: target });
    }
    let rate = if host.playing && (0.3..=1.5).contains(&drift.abs()) {
        if drift > 0.0 {
            1.05
        } else {
            0.95
        }
    } else {
        1.0
    };
    if (local.playback_rate - rate).abs() > 0.001 {
        result.commands.push(PlayerCommand::Rate { rate });
    }
    if local.playing != host.playing {
        result.commands.push(if host.playing {
            PlayerCommand::Play
        } else {
            PlayerCommand::Pause
        });
    }
    if drift.abs() > 0.3 {
        result.status = SyncStatus::Behind;
    }
    result
}

pub fn host_frame(
    local: &PlaybackState,
    previous: Option<&SyncFrame>,
    now: f64,
    offset: f64,
) -> Option<SyncFrame> {
    let server_now = now + offset;
    if local.ad {
        // Never publish an advertisement ID. Freeze the last music timeline once.
        let previous = previous?;
        let position = previous.position
            + if previous.playing {
                ((server_now - previous.host_ts) / 1000.0).max(0.0)
            } else {
                0.0
            };
        return Some(SyncFrame {
            position,
            playing: false,
            host_ts: server_now,
            ..previous.clone()
        });
    }
    let track = local.track.as_ref()?;
    let playing = local.playing && !local.buffering;
    let position = local.position
        + if playing {
            ((now - local.ts) / 1000.0).max(0.0) * local.playback_rate
        } else {
            0.0
        };
    Some(SyncFrame {
        video_id: track.video_id.clone(),
        position,
        playing,
        host_ts: server_now,
        seq: 0,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use ytm_protocol::TrackInfo;
    fn fixtures() -> (PlaybackState, SyncFrame) {
        (
            PlaybackState {
                track: Some(TrackInfo {
                    video_id: "dQw4w9WgXcQ".into(),
                    ..Default::default()
                }),
                position: 10.0,
                duration: 200.0,
                playing: true,
                playback_rate: 1.0,
                ts: 1000.0,
                ..Default::default()
            },
            SyncFrame {
                video_id: "dQw4w9WgXcQ".into(),
                position: 10.0,
                playing: true,
                host_ts: 1000.0,
                seq: 1,
            },
        )
    }
    #[test]
    fn corrects_drift_and_uses_both_clocks() {
        let (local, mut host) = fixtures();
        assert!(decide(&local, &host, 2000.0, 0.0).commands.is_empty());
        host.position = 11.0;
        assert_eq!(
            decide(&local, &host, 1000.0, 0.0).commands,
            vec![PlayerCommand::Rate { rate: 1.05 }]
        );
        host.position = 8.0;
        assert_eq!(
            decide(&local, &host, 1000.0, 0.0).commands,
            vec![PlayerCommand::Seek { position: 8.0 }]
        );
        host.host_ts = 1500.0;
        host.position = 10.0;
        assert!(decide(&local, &host, 1000.0, 500.0).commands.is_empty());
    }
    #[test]
    fn pauses_seeks_and_waits_for_ads() {
        let (mut local, mut host) = fixtures();
        host.playing = false;
        host.position = 11.0;
        assert_eq!(
            decide(&local, &host, 1000.0, 0.0).commands,
            vec![PlayerCommand::Seek { position: 11.0 }, PlayerCommand::Pause]
        );
        local.ad = true;
        local.playback_rate = 1.05;
        let d = decide(&local, &host, 1000.0, 0.0);
        assert_eq!(d.status, SyncStatus::Ad);
        assert_eq!(d.commands, vec![PlayerCommand::Rate { rate: 1.0 }]);
    }
    #[test]
    fn changes_tracks_during_buffering_and_starts_a_paused_video() {
        let (mut local, mut host) = fixtures();
        local.buffering = true;
        local.playing = false;
        assert_eq!(
            decide(&local, &host, 1000.0, 0.0).commands,
            vec![PlayerCommand::Play]
        );
        host.video_id = "abcdefghijk".into();
        assert_eq!(
            decide(&local, &host, 1000.0, 0.0).commands,
            vec![PlayerCommand::LoadTrack {
                video_id: "abcdefghijk".into()
            }]
        );
    }
    #[test]
    fn host_ads_freeze_music_instead_of_publishing_an_ad() {
        let (mut local, host) = fixtures();
        local.ad = true;
        local.track.as_mut().unwrap().video_id = "advertisemt".into();
        assert!(host_frame(&local, None, 1000.0, 0.0).is_none());
        let frozen = host_frame(&local, Some(&host), 3000.0, 0.0).unwrap();
        assert_eq!(frozen.video_id, host.video_id);
        assert_eq!(frozen.position, 12.0);
        assert!(!frozen.playing);
        assert_eq!(
            host_frame(&local, Some(&frozen), 6000.0, 0.0)
                .unwrap()
                .position,
            12.0
        );
    }
}
