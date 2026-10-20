# Remote control, group control, and Android

Sharing starts disabled on every launch. Open **Remote control** for persistent personal device pairing, or **Group control** for guest admission that lasts only for the current session. Both control one host player; controllers do not receive audio. **Listen together** remains a separate mode in which each member plays audio locally. Leave/disconnect/end the current mode explicitly before switching.

## Hosting and joining

1. End any listening room or remote connection.
2. Start personal or group sharing. The host listens on IPv4 TCP port **9863**. If another app/profile occupies that port, starting sharing fails visibly.
3. Select a usable interface address, copy the invitation or scan its QR code. Allow the app through the host firewall on the trusted local network. VPN and virtual interfaces may also appear; select the interface reachable from the controller. No discovery or router forwarding is required.
4. In the installed app, tap the **device button in the player bar** and select a host under **Nearby devices**. Native mDNS/DNS-SD (`_ytmlisten._tcp.local.`, UDP 5353) advertises enabled sharing hosts. The picker verifies public session identity at the advertised private IPv4 endpoint, filters out this device, and lists verified hosts as they arrive during an eight-second scan. Opening the picker starts discovery; it refreshes every twenty seconds while open, and **Refresh nearby devices** starts another scan. Android holds a multicast lock during a scan or while hosting, then releases it. Discovery includes name, protocol version, session ID, mode, and LAN addresses; invitation secrets and device credentials are never advertised. Selection uses normal host approval and saved personal pairing credentials.

   QR scanning, full invitation links, and LAN IP/port entry (for example, `192.168.1.10:9863`) remain available if the network isolates clients or blocks multicast. Allow the host app on the trusted private network, including UDP 5353 and TCP 9863. Address-only joining requires an updated host. Approved personal LAN devices remain in the picker for future reconnects. Internet relay connections require the full invitation link. Browser guests can still open invitations in their browser.

5. The host can change each participant's permissions, remove/revoke devices, rotate invitations, or end sharing. Volume and replacing/clearing the queue are disabled for guests by default.

Installed controllers use the normal player, search, and queue after selecting a playback device. The host's track, playback position, transport and queue replace the local playback view; local account/settings remain local. Play/pause, skip and seek use the native Rust command path. Android notification, lock-screen and Bluetooth controls use the same path through MediaSession; remote mode takes no local audio focus and never starts local audio. Permissions apply to both app and system controls. Disconnect explicitly to return to this device; loss of the host disables controls without switching playback automatically.

Signed-in controllers browse their own account's library locally. Unsigned controllers use the host's anonymous public search and public collection browsing through the same app screens. Private library/home requests never proxy to the host. **Play now**, **Play next**, and **Add to queue** send selected recordings to the host. Selecting a collection track queues up to 50 loaded tracks from that position; large Play all selections show **Play first 50**. Group Play now preserves the upcoming queue. Playing the existing host queue selects its first entry without inserting duplicates. Queue select/remove/move use stable entry IDs and revisions; partial pages never imply an absolute end position until the end is loaded. Shuffle and repeat remain host-side settings. Browser guests retain the standalone browser interface. Public catalog pages are capped at 500 source items, returned in pages of up to 50 items and 48 KB of metadata. Queue pages have the same response bounds, with a 5,000-entry queue limit.

Personal credentials persist in the controller's browser storage or app store. The host stores only their SHA-256 hashes and granted permissions. Revoking a device removes its saved pairing and current admission. Group credentials survive connection loss only within the live session; a host restart creates a new session. A changed LAN address/browser origin may require pairing that browser again. Installed app credentials are associated with the invitation origin.

LAN HTTP/WebSocket traffic is intended for trusted networks. Invitation secrets travel in the URL fragment and are removed from browser history after loading; authentication then uses WebSocket messages. The host validates Host and browser Origin, bounds message sizes and connection counts, requires approval before reading state or executing commands, and limits each controller to 40 messages per ten seconds. A session has at most 20 controllers including pending/disconnected admissions; hosts can remove old entries to free slots.

## Cloudflare deployment

Build/install dependencies, then deploy the existing relay:

```sh
pnpm install --frozen-lockfile
pnpm --filter @ytm/relay exec wrangler deploy
```

The configuration retains the `ROOM` binding and adds the independent `CONTROL` binding and `v2-control` SQLite Durable Object migration. Static assets come from `apps/controller`, which is also bundled into the Rust LAN server. `/room` and `/join` behavior is unchanged. Control uses `/control/<session-id>` and the browser interface is at `/controller/`.

Configure the HTTPS relay origin in **Listen together**, save it, and enable the Cloudflare option when starting sharing. The host makes an outbound WSS connection; do not expose the LAN server through a router. Playback and queue state remain authoritative on the live host. Hibernating WebSocket attachments retain routing and reconnect hello metadata; storage retains the host credential and disconnect deadline. A reconnect re-admits existing sockets through the same live host. Controls disable on host loss; sessions expire after ten minutes without the host, with no host transfer. Expired relay session IDs remain tombstoned to prevent reuse. End sharing closes relay controllers immediately while the host is connected.

The relay can see invitation/device authentication messages, participant names, request IDs, permissions, track metadata, playback position, volume, queue edits and anonymous public catalog responses. It carries no audio, Google cookies, Google authorization headers, host settings, or private-library responses. Observability remains disabled by default. Run a relay you trust; HTTPS protects transport, not data from its operator.

Mutations share the host's asynchronous lock with local playback and queue edits. Permissions are checked when execution begins, and playback replies retain the existing bridge acknowledgement. Stable queue-entry IDs distinguish duplicate recordings, and revisions reject stale edits. Each controller has a bounded history of 512 mutation requests per session; retries return the stored result, including uncertain failures, instead of reapplying the command. Reusing an ID for a different command fails. Start a new session when the history fills. Reconnecting clients fetch state and never resend uncertain commands automatically.

## Android feasibility build

Android is part of the full app: the same Svelte library/player, LAN host, relay host, and installed controller code are used. Desktop tray, shortcuts, Discord, window blur and mini-player modules/dependencies are excluded from Android. The native plugin in `crates/android-player` owns a YouTube Music WebView in a foreground media-playback service, an origin-scoped WebMessage bridge, notification/media-session controls, audio focus, a playback wake lock, and audio-output-disconnect handling. Sign-in uses the same WebView session in an account dialog. Audio stays on the selected system output; no audio URL extraction or controller streaming is used. Windows monitors the player's active audio endpoints and pauses if an endpoint disappears.

Use JDK 17+, Android SDK/platform 36, NDK 28.2.13676358, and the Rust Android target:

```sh
rustup target add aarch64-linux-android
pnpm --filter @ytm/desktop tauri android build --debug --target aarch64 --apk
```

Set `ANDROID_HOME` and `NDK_HOME` for your machine. The checked-in Android project lives under `apps/desktop/src-tauri/gen/android`; do not regenerate it over the maintained manifest/build/signing configuration. The build outputs APKs under its `app/build/outputs/apk` directory. CI runs the same build and uploads an arm64 **feasibility** APK. Install on Android 8/API 26 or newer with an updated Android System WebView.

**Android hosting is not accepted until the physical-device checks below pass.** A cross-compiled library or APK does not establish successful Google sign-in, background WebView behavior or uninterrupted playback. This implementation session had an SDK but `adb devices` reported no device. No locked-screen or cross-device pass is claimed.

Device testing of the initial release found that Google rejects sign-in with a “browser not secure” message. The embedded WebView account flow is a known blocker for Android account/library support. External browser sign-in does not populate the service WebView's cookie store; changing the user-agent is not a supported authentication solution. Android remote control of an authenticated desktop host does not require Android Google sign-in. Full Android account support remains incomplete pending an authentication/playback design that supports Google's browser policy.

Record device model, Android/WebView versions, network, build SHA, elapsed lock time, and observed results for:

- Google sign-in and restart persistence; public search and private library browsing.
- Playback through screen lock and natural track advancement for at least two tracks; repeat and queue behavior.
- Android-host/browser-controller and desktop-host/Android-controller over LAN and separate internet connections.
- Remote play/pause, skip, seek and queue editing while the host remains locked.
- Notification controls, lock-screen controls and Bluetooth media buttons.
- Audio interruption/focus loss, calls and headphone/Bluetooth disconnection; verify playback pauses and does not unexpectedly resume on speakers.
- Wi-Fi changes, relay reconnection, host loss, ten-minute expiry, service/process death and a fresh group session after restart.
- Permission denial for notifications, outdated WebView, failed sign-in, service startup errors and explicit disconnect/end sharing.

Release APK signing uses maintainer-provided `YTM_ANDROID_KEYSTORE`, `YTM_ANDROID_STORE_PASSWORD`, `YTM_ANDROID_KEY_ALIAS`, and `YTM_ANDROID_KEY_PASSWORD`. The Android workflow's release option requires `ANDROID_ACCEPTANCE_PASSED=true` and the corresponding `ANDROID_*` repository secrets, including `ANDROID_KEYSTORE_BASE64`. It uploads a signed APK artifact; it does not publish a release. No signing credentials are included in source, and debug builds do not require them.

## Automated verification

Run `pnpm check`, `pnpm lint`, `pnpm test`, `pnpm build`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, and `pnpm --filter @ytm/relay exec wrangler deploy --dry-run --outdir dist`.

Control tests cover admission, pairing persistence, permission denial, revocation, invitation rotation, bounded admission, strict operation decoding, deduplicated request identity, stale queue revisions, duplicate track identities, queue preservation, snapshot privacy, browser metadata escaping, uncertain-command handling, and anonymous catalog requests. Relay tests cover route/origin isolation, host-only routing, reconnect attachments, host loss, authenticated reconnect, expiry, capacity and existing listening-room behavior. Manual hardware/network acceptance remains distinct from these checks.

For an actual Windows LAN server check without audible playback, run `cargo run -p ytm-listen -- --profile control-smoke --check-control`. It starts an isolated host, exercises real HTTP/WebSocket connections, admission, permission denial, duplicate suppression, two concurrent controllers editing the same revision, reconnect, revocation and end sharing, prints `CONTROL_CHECK`, and exits. The isolated profile prevents access to the default Google session. This check passed during implementation, as did the arm64 debug APK build. Physical Android and separate-network checks remain pending.
