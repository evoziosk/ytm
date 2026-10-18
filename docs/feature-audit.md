# Feature audit, 18 October 2026

Google sign-in and Premium playback have been confirmed working by the user.
The priorities below focus on everyday listening now that those flows work.

## Added in 0.3.1

**Preferences > Appearance > Window blur** enables a translucent main window
with native Windows desktop blur. It applies and saves immediately, can be
switched off, and is restored on launch. The default remains opaque. Changing
appearance preserves other unsaved preferences and the playback session.

## Recommended next features

| Priority | Feature                              | Benefit and current gap                                                                                                                                                                                                                                     |
| -------- | ------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1        | Play next / Add to queue             | Choosing a result currently replaces the queue. Track menus should let people build a listening session without interrupting the current song. Offer the same actions from quick search.                                                                    |
| 2        | Restore the listening session        | The queue, shuffle, repeat and player state currently start fresh. Restore the queue, selected track, position and volume after restart, with playback paused until the user resumes. Include a choice to disable session history and clear it on sign-out. |
| 3        | Library and playlist actions         | Browsing is available, but there is no app-owned playlist editor. Add save/unsave, create playlist, add/remove tracks, and clear feedback when an account or playlist is read-only.                                                                         |
| 4        | Sleep timer and stop after this song | Add a small timer with remaining time and cancellation, plus an option to stop at the current track's end. These controls fit both the player and tray.                                                                                                     |
| 5        | Automatic lyrics                     | Lyrics currently require a lookup. Add an opt-in automatic lookup, caching, a clear instrumental/not-found state, and track/version matching. Keep a manual refresh action.                                                                                 |
| 6        | Faster queue management              | Up/down buttons work, but long queues need drag and drop, multi-select removal, and a reversible shuffle that can restore the previous order.                                                                                                               |
| 7        | Shortcut status and recording        | Report which shortcuts actually registered. A key-recording field and a conflict explanation would make setup easier than typing combinations. Startup currently reports registration failures in logs.                                                     |
| 8        | Open music links                     | Accept pasted YouTube Music song, album and playlist links, and provide Copy song link on tracks. The existing custom URL handler currently handles listening-room invitations.                                                                             |

The first two would make the biggest difference to everyday use. Playlist
editing is the next account-focused improvement.

## Code reviewed

- `apps/desktop/src-tauri/src/catalog.rs`: catalog queries and queue replacement.
- `apps/desktop/src-tauri/src/queue.rs`: current queue operations and shuffle.
- `apps/desktop/src-tauri/src/state.rs`: fresh queue/player state on startup.
- `apps/desktop/bridge/src/catalog.ts`: search/browse/account requests.
- `apps/desktop/ui/Music.svelte`: browsing and track actions.
- `apps/desktop/ui/QuickSearch.svelte`: overlay search and playback.
- `apps/desktop/src-tauri/src/lyrics.rs`: manual lyric lookup.
- `apps/desktop/src-tauri/src/shortcuts.rs`: shortcut registration.
- `apps/desktop/src-tauri/src/lib.rs`: room-link handling.
