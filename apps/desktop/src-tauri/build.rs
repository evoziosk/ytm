fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "player_event",
            "get_snapshot",
            "send_command",
            "retry_playback",
            "dismiss_playback_error",
            "queue_action",
            "save_settings",
            "set_window_blur",
            "open_panel",
            "show_music",
            "party_connect",
            "party_leave",
            "party_transfer",
            "get_lyrics",
            "music_query",
            "catalog_result",
            "play_items",
            "open_account",
            "logout_account",
            "refresh_music",
        ]),
    ))
    .expect("Tauri build configuration");
}
