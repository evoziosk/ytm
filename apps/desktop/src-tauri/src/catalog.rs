use crate::state::Core;
use tauri::{AppHandle, Manager, State, WebviewWindow};
use ytm_protocol::{
    valid_video_id, CatalogKind, CatalogPage, CatalogReply, CatalogRequest, MusicItem,
    PlayerCommand, Role,
};

fn valid_browse(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn validate_item(item: &MusicItem) -> bool {
    item.title.len() <= 2048
        && item.artist.len() <= 2048
        && item.subtitle.len() <= 2048
        && item.key.len() <= 256
        && item.kind.len() <= 32
        && item.artwork.len() <= 2048
        && (item.video_id.is_empty() || valid_video_id(&item.video_id))
        && (item.browse_id.is_empty() || valid_browse(&item.browse_id))
        && (item.artwork.is_empty()
            || url::Url::parse(&item.artwork).is_ok_and(|url| {
                url.scheme() == "https"
                    && url.host_str().is_some()
                    && url.username().is_empty()
                    && url.password().is_none()
            }))
}
#[tauri::command]
pub async fn music_query(
    app: AppHandle,
    core: State<'_, Core>,
    request: CatalogRequest,
) -> Result<CatalogPage, String> {
    query(&app, &core, request).await
}
pub async fn query(
    app: &AppHandle,
    core: &Core,
    request: CatalogRequest,
) -> Result<CatalogPage, String> {
    for _ in 0..60 {
        if core.snapshot().bridge_connected {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    if !core.snapshot().bridge_connected {
        return Err(
            "The playback service has not loaded. Check your connection in Account, then retry."
                .into(),
        );
    }
    if request.query.len() > 512
        || request.continuation.len() > 32768
        || matches!(request.kind, CatalogKind::Search) && request.query.trim().is_empty()
        || matches!(request.kind, CatalogKind::Browse) && !valid_browse(&request.browse_id)
    {
        return Err("Invalid music request".into());
    }
    let main = app
        .get_webview_window("main")
        .ok_or("Playback service is unavailable")?;
    if main
        .url()
        .map_err(|e| e.to_string())?
        .origin()
        .ascii_serialization()
        != "https://music.youtube.com"
    {
        return Err("Finish account sign-in, then return to your music.".into());
    }
    let id = {
        use rand::Rng;
        let mut rng = rand::rng();
        (0..16)
            .map(|_| format!("{:02x}", rng.random::<u8>()))
            .collect::<String>()
    };
    let (sender, receiver) = tokio::sync::oneshot::channel();
    {
        let mut pending = core.catalog_pending.lock().expect("catalog lock");
        if pending.len() >= 8 {
            return Err("Please wait for the current music request".into());
        }
        pending.insert(id.clone(), sender);
    }
    let payload = serde_json::to_string(&request).map_err(|e| e.to_string())?;
    if let Err(error) = main.eval(format!("window.__YTM_LISTEN_QUERY__?.({id:?}, {payload})")) {
        core.catalog_pending
            .lock()
            .expect("catalog lock")
            .remove(&id);
        return Err(error.to_string());
    }
    let result = tokio::time::timeout(std::time::Duration::from_secs(20), receiver).await;
    core.catalog_pending
        .lock()
        .expect("catalog lock")
        .remove(&id);
    result
        .map_err(|_| {
            "The music service did not respond. Open Account to check its connection, then retry."
                .to_string()
        })?
        .map_err(|_| "Music request cancelled".to_string())?
}
#[tauri::command]
pub fn catalog_result(
    app: AppHandle,
    window: WebviewWindow,
    core: State<Core>,
    reply: CatalogReply,
) -> Result<(), String> {
    if window.label() != "main"
        || window
            .url()
            .map_err(|e| e.to_string())?
            .origin()
            .ascii_serialization()
            != "https://music.youtube.com"
    {
        return Err("Invalid catalog origin".into());
    }
    if reply.id.len() != 32
        || reply.error.len() > 2048
        || reply.page.as_ref().is_some_and(|page| {
            page.title.len() > 2048
                || page.message.len() > 2048
                || page.items.len() > 500
                || page.continuation.len() > 32768
                || page.playlist_id.len() > 256
                || page.items.iter().any(|item| !validate_item(item))
        })
    {
        return Err("Invalid catalog reply".into());
    }
    let sender = core
        .catalog_pending
        .lock()
        .expect("catalog lock")
        .remove(&reply.id)
        .ok_or("No pending request")?;
    let result = reply.page.ok_or(reply.error);
    if let Ok(page) = &result {
        core.data.write().expect("state lock").account_message = if page.signed_in {
            "Signed in to YouTube Music"
        } else {
            "Listening without an account"
        }
        .into();
        core.emit(&app);
    }
    let _ = sender.send(result);
    Ok(())
}
#[tauri::command]
pub async fn play_items(
    app: AppHandle,
    core: State<'_, Core>,
    items: Vec<MusicItem>,
    index: usize,
    playlist_id: String,
) -> Result<(), String> {
    if core.snapshot().party.role == Some(Role::Member) {
        return Err("Leave your listening party to choose another track".into());
    }
    if items.is_empty()
        || items.len() > 5000
        || (!playlist_id.is_empty() && !valid_browse(&playlist_id))
        || index >= items.len()
        || items
            .iter()
            .any(|item| !validate_item(item) || !valid_video_id(&item.video_id))
    {
        return Err("Invalid play queue".into());
    }
    let id = items[index].video_id.clone();
    {
        let mut data = core.data.write().expect("state lock");
        data.previous_playlist = data
            .player
            .track
            .as_ref()
            .map(|track| (track.video_id.clone(), track.playlist_id.clone()));
        data.queue = items;
        data.queue_index = index;
        data.queue_playlist_id = crate::settings::playlist_id(&playlist_id);
        if data.shuffle {
            use rand::seq::SliceRandom;
            data.queue[index + 1..].shuffle(&mut rand::rng());
        }
    }
    core.emit(&app);
    crate::playback::confirmed(&app, PlayerCommand::LoadTrack { video_id: id }).await
}
pub async fn advance(app: &AppHandle, forward: bool, ended: bool) -> Result<bool, String> {
    let core = app.state::<Core>();
    let state = core.snapshot();
    if state.queue.is_empty() || state.party.role == Some(Role::Member) {
        return Ok(false);
    }
    let Some(index) = crate::queue::next_index(
        state.queue.len(),
        state.queue_index,
        forward,
        ended,
        &state.repeat,
    ) else {
        crate::playback::confirmed(app, PlayerCommand::Pause).await?;
        return Ok(true);
    };
    let item = &state.queue[index];
    core.data.write().expect("state lock").queue_index = index;
    core.emit(app);
    if index == state.queue_index {
        crate::playback::confirmed(app, PlayerCommand::Seek { position: 0.0 }).await?;
        crate::playback::confirmed(app, PlayerCommand::Play).await?;
        return Ok(true);
    }
    crate::playback::confirmed(
        app,
        PlayerCommand::LoadTrack {
            video_id: item.video_id.clone(),
        },
    )
    .await?;
    Ok(true)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_catalog_injection_and_unsafe_artwork() {
        let mut item = MusicItem {
            key: "dQw4w9WgXcQ".into(),
            title: "Song".into(),
            subtitle: String::new(),
            artist: String::new(),
            artwork: "https://example.com/art".into(),
            kind: "song".into(),
            video_id: "dQw4w9WgXcQ".into(),
            browse_id: String::new(),
        };
        assert!(validate_item(&item));
        item.artwork = "file:///C:/secret".into();
        assert!(!validate_item(&item));
        assert!(!valid_browse("../../config"));
    }
}
