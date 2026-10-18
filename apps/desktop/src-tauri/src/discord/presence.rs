use crate::{
    settings::Settings,
    state::{now_ms, Core},
};
use discord_rich_presence::{
    activity::{Activity, ActivityType, Assets, Button, StatusDisplayType, Timestamps},
    DiscordIpc, DiscordIpcClient,
};
use tauri::AppHandle;
use ytm_protocol::{PlaybackState, Role};

pub fn timestamps(now: f64, position: f64, duration: f64) -> (i64, i64) {
    let start = (now - position.max(0.0) * 1000.0) as i64;
    (start, start + (duration.max(0.0) * 1000.0) as i64)
}
fn update_due(now: f64, last_sent: f64, active: bool, changed: bool, drift: f64) -> bool {
    now - last_sent >= 4100.0 && (!active || changed || drift.abs() > 2.0)
}
fn visible(settings: &Settings, player: &PlaybackState, idle: f64) -> bool {
    settings.discord_enabled
        && !settings.incognito
        && !player.ad
        && player.track.as_ref().is_some_and(|track| {
            !(settings.hide_paused && !player.playing
                || settings.idle_minutes > 0 && idle >= f64::from(settings.idle_minutes) * 60000.0
                || settings
                    .blocked_artists
                    .iter()
                    .any(|s| s.eq_ignore_ascii_case(&track.artist))
                || settings.blocked_playlists.iter().any(|value| {
                    crate::settings::playlist_id(value)
                        == crate::settings::playlist_id(&track.playlist_id)
                }))
        })
}
fn status(app: &AppHandle, core: &Core, text: &str) {
    {
        let mut d = core.data.write().expect("state lock");
        if d.discord == text {
            return;
        }
        d.discord = text.into();
    }
    core.emit(app);
}
pub fn start(app: &AppHandle, core: &Core) {
    let app = app.clone();
    let core = core.clone();
    std::thread::spawn(move || {
        let mut client: Option<DiscordIpcClient> = None;
        let mut app_id = String::new();
        let mut last_attempt = 0.0;
        let mut last_sent = 0.0;
        let mut active = false;
        let mut signature = String::new();
        let mut last_position = 0.0;
        let mut last_observed = now_ms();
        let mut last_playing = now_ms();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let snapshot = core.snapshot();
            let settings = &snapshot.settings;
            let player = &snapshot.player;
            let now = now_ms();
            if player.playing {
                last_playing = now;
            }
            if settings.discord_app_id != app_id {
                if let Some(mut old) = client.take() {
                    let _ = old.clear_activity();
                    let _ = old.close();
                }
                app_id = settings.discord_app_id.clone();
                last_attempt = 0.0;
                active = false;
                signature.clear();
            }
            if app_id.is_empty() || !settings.discord_enabled {
                if active {
                    if let Some(mut c) = client.take() {
                        let _ = c.close();
                    }
                    last_attempt = now;
                    active = false;
                }
                status(
                    &app,
                    &core,
                    if app_id.is_empty() {
                        "Not configured"
                    } else {
                        "Disabled"
                    },
                );
                continue;
            }
            let show = visible(settings, player, now - last_playing);
            if client.is_none() && now - last_attempt >= 15000.0 {
                last_attempt = now;
                let mut connection = DiscordIpcClient::new(&app_id);
                match connection.connect().map(|()| connection) {
                    Ok(c) => {
                        client = Some(c);
                        signature.clear();
                        active = false;
                        status(&app, &core, "Connected");
                    }
                    Err(_) => {
                        status(&app, &core, "Waiting for Discord");
                        continue;
                    }
                }
            }
            let Some(c) = client.as_mut() else {
                continue;
            };
            // Close the connection for immediate privacy when a clear would exceed the update cadence.
            if !show {
                if active {
                    if now - last_sent < 4100.0 {
                        let _ = c.close();
                        client = None;
                        last_attempt = now;
                    } else if c.clear_activity().is_err() {
                        client = None;
                    }
                    active = false;
                    signature.clear();
                    last_sent = now;
                }
                status(
                    &app,
                    &core,
                    if settings.incognito {
                        "Incognito"
                    } else {
                        "Hidden"
                    },
                );
                continue;
            }
            let Some(track) = &player.track else {
                continue;
            };
            let key = format!(
                "{}|{}|{}|{}|{}|{}|{:?}|{}",
                track.video_id,
                track.title,
                track.artist,
                track.artwork,
                player.playing,
                snapshot.party.invite_url,
                snapshot.party.role,
                snapshot.party.connected
            );
            let expected = last_position
                + if player.playing {
                    (now - last_observed) / 1000.0
                } else {
                    0.0
                };
            let position = player.position
                + if player.playing {
                    ((now - player.ts) / 1000.0).max(0.0)
                } else {
                    0.0
                };
            if !update_due(
                now,
                last_sent,
                active,
                key != signature,
                position - expected,
            ) {
                continue;
            }
            let link = format!("https://music.youtube.com/watch?v={}", track.video_id);
            let artist = if player.playing {
                track.artist.clone()
            } else {
                format!("Paused · {}", track.artist)
            };
            let mut activity = Activity::new()
                .name("YouTube Music")
                .activity_type(ActivityType::Listening)
                .status_display_type(StatusDisplayType::Details)
                .details(&track.title)
                .state(&artist);
            if !track.artwork.is_empty() {
                activity = activity.assets(Assets::new().large_image(&track.artwork).large_text(
                    if track.album.is_empty() {
                        "YouTube Music"
                    } else {
                        &track.album
                    },
                ));
            }
            if player.playing && player.duration > 0.0 {
                let (start, end) = timestamps(now, position, player.duration);
                activity = activity.timestamps(Timestamps::new().start(start).end(end));
            }
            let mut buttons = vec![Button::new("Open in YouTube Music", &link)];
            if snapshot.party.role == Some(Role::Host)
                && snapshot.party.connected
                && !snapshot.party.invite_url.is_empty()
            {
                buttons.push(Button::new(
                    "Join listening party",
                    &snapshot.party.invite_url,
                ));
            }
            activity = activity.buttons(buttons);
            last_sent = now;
            match c.set_activity(activity) {
                Ok(()) => {
                    active = true;
                    signature = key;
                    last_position = position;
                    last_observed = now;
                    status(&app, &core, "Sharing your music");
                }
                Err(_) => {
                    client = None;
                    active = false;
                    status(&app, &core, "Waiting for Discord");
                }
            }
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presence_coalesces_changes_and_respects_update_budget() {
        assert!(!update_due(4000.0, 0.0, true, true, 30.0));
        assert!(update_due(4100.0, 0.0, true, true, 0.0));
        assert!(!update_due(9000.0, 0.0, true, false, 1.0));
        assert!(update_due(9000.0, 0.0, true, false, 2.1));
        let mut last = -4100.0;
        let mut count = 0;
        for now in (0..20000).step_by(500) {
            if update_due(f64::from(now), last, true, true, 0.0) {
                count += 1;
                last = f64::from(now);
            }
        }
        assert!(count <= 5);
    }
    #[test]
    fn timestamps_use_milliseconds_and_track_progress() {
        assert_eq!(timestamps(1_000_000.0, 30.0, 180.0), (970_000, 1_150_000));
    }
    #[test]
    fn incognito_and_blocklists_hide_presence() {
        let mut settings = Settings::default();
        let mut player = PlaybackState {
            playing: true,
            ..Default::default()
        };
        player.track = Some(ytm_protocol::TrackInfo {
            artist: "Secret Artist".into(),
            ..Default::default()
        });
        assert!(visible(&settings, &player, 0.0));
        settings.blocked_artists.push("secret artist".into());
        assert!(!visible(&settings, &player, 0.0));
        settings.blocked_artists.clear();
        settings.incognito = true;
        assert!(!visible(&settings, &player, 0.0));
    }
}
