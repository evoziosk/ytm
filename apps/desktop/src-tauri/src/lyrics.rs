use crate::state::Core;
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Lyrics {
    synced_lyrics: Option<String>,
    plain_lyrics: Option<String>,
    instrumental: bool,
}
#[tauri::command]
pub async fn get_lyrics(core: State<'_, Core>) -> Result<Lyrics, String> {
    let remote = crate::control_client::status(&core);
    let player = if remote.active {
        let snapshot = remote.snapshot.ok_or("Waiting for the playback device")?;
        ytm_protocol::PlaybackState {
            track: snapshot.track.map(|track| ytm_protocol::TrackInfo {
                video_id: track.video_id,
                title: track.title,
                artist: track.artist,
                artwork: track.artwork,
                ..Default::default()
            }),
            duration: snapshot.duration,
            ..Default::default()
        }
    } else {
        core.snapshot().player
    };
    let track = player.track.ok_or("Play a song to look up lyrics")?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .user_agent("YTMListen/0.2.2 (desktop lyrics lookup)")
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get("https://lrclib.net/api/get")
        .query(&[
            ("track_name", track.title),
            ("artist_name", track.artist),
            ("duration", player.duration.round().to_string()),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err("No lyrics found for this song".into());
    }
    let response = response.error_for_status().map_err(|e| e.to_string())?;
    if response
        .content_length()
        .is_some_and(|length| length > 512000)
    {
        return Err("Lyrics response too large".into());
    }
    response.json().await.map_err(|e| e.to_string())
}
