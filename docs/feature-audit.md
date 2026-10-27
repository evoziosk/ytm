# Feature audit, 27 October 2026

This audit covers desktop browsing, queue, popup search, lyrics, and remote
controller flows, together with the existing bridge, protocol, relay, and Rust
tests. Google sign-in and Premium playback were previously confirmed by the
user; they were not repeated against a live account during this audit.

## Implemented in the working tree

The application icon now uses the interface's rose/plum gradient and light
waveform bars. Desktop PNG, Windows ICO, macOS ICNS, and Android launcher assets
were regenerated from the updated SVG; the Windows tray and volume mixer use
the same packaged icon.

| Area              | Finding                                                                                                                                       | Change                                                                                                                                                                                                                                                                                                        |
| ----------------- | --------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Popup search      | Starting playback was the only action, so building a queue required returning to the main window.                                             | Add **Play next** and **Add to queue** for the selected result. Shift+Enter plays next; Ctrl+Enter adds to the end. Successful additions keep the popup open and acknowledge the selected song. Errors remain visible for retry. Backend room locks and host permission checks still apply.                   |
| Lyrics requests   | A rejected request for a previous song could replace the new song's status. Returning to the same song could also admit an obsolete response. | Scope requests to a generation and the active playback source, song metadata, and duration. Track changes immediately enable lookup for the new song; obsolete successes, errors, and completion callbacks cannot overwrite it.                                                                               |
| Remote lyrics     | The window cleared lyrics using local player updates, even while displaying a remote host's track.                                            | Give the lyrics panel the same effective playback view as the player, and reset only when its lookup identity changes. Ordinary position and buffering updates retain lyrics.                                                                                                                                 |
| Automatic lyrics  | Every track needed a manual lookup.                                                                                                           | Add an opt-in **Automatically find lyrics** switch, a bounded cache of 20 successful results, instrumental feedback, and **Refresh lyrics** to bypass the cache. Automatic lookup defaults off when the panel is created; the cache lives only in that panel's memory. Advertisements do not trigger lookups. |
| Queue permissions | Controllers with queue permission but no transport permission had **Play next** and **Add to queue** incorrectly disabled.                    | Enable those actions with queue permission, keeping **Play now** and track selection subject to transport permission. Host validation remains authoritative.                                                                                                                                                  |
| Song sharing      | There was no direct way to copy the current song's link.                                                                                      | Add **Copy song link** to the player for local and remote tracks, with success feedback and visible clipboard errors. The URL contains only the song identifier, without private collection context.                                                                                                          |

## Verification

- Desktop tests cover popup queue placement and retry, lyrics request races
  (including A to B to A), ordinary player updates, remote-source changes,
  automatic opt-in, caching, refresh, instrumental responses, and clipboard errors.
- Existing playback, account, queue, remote controller, bridge, protocol, and Rust
  tests are included in verification. Native loopback discovery tests require
  execution outside the Windows sandbox. The multicast hardware smoke test
  remains explicitly ignored by the existing suite.
- Type checking and production builds cover the interface and injected player
  bridge. Browser smoke checks use mocked native commands and song data; they
  do not establish live YouTube playback or physical-device acceptance.
- The Cloudflare relay suite cannot initialize its internal test module inside
  the sandbox, but passes outside it. No relay dependency change is needed.

## Remaining priorities

| Priority | Feature                       | Benefit and gap                                                                                                                                                                                               |
| -------- | ----------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1        | Restore the listening session | Persist queue, selected track, position, volume, shuffle, and repeat. Resume paused, offer a history opt-out, and clear saved session data on sign-out. Startup still creates a fresh queue and player state. |
| 2        | Library and playlist editing  | Add save/unsave, playlist creation, and add/remove actions with clear account and read-only feedback. This requires authenticated service and Android API design beyond read-only browsing.                   |
| 3        | Sleep timer                   | Add elapsed-time and end-of-song stopping, cancellation, and a shared countdown that continues while the window is hidden.                                                                                    |
| 4        | Larger-queue editing          | Add drag and drop, multi-select removal, and reversible shuffle while preserving stable entry identifiers and remote queue revisions.                                                                         |
| 5        | Shortcut recording and status | Surface actual registration results, support key recording, and explain conflicts. Startup failures are primarily visible in logs.                                                                            |
| 6        | Open music links              | Accept pasted song, album, and playlist links. Copying the current song's link is now available; the custom URL handler still focuses on room invitations.                                                    |
| 7        | Lyrics version matching       | Improve track/version matching and consider a user-controlled durable cache. The new cache is temporary; matching still relies on LRCLIB's title, artist, and duration lookup.                                |
| 8        | Hardware acceptance           | Complete virtual-microphone cable/mic acceptance and Android native-library and authenticated/background playback testing. These require physical devices and configured accounts.                            |

Session restoration is the next larger feature. It needs coordinated persistence
and a playback handshake rather than automatically replaying saved commands at
startup. The implemented changes can be reviewed independently of that work.
