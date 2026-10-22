# Changelog

## 0.5.1 — 2026-10-22

- Fix the Linux AppImage not playing any music: bundle the GStreamer media plugins WebKitGTK needs, and install them in the release build.

## 0.5.0 — 2026-10-22

- Introduce the plum/rose translucent desktop theme and redesign music, preferences, sharing and connection panels.
- Add the desktop discovery layout with global header search, spotlight picks, grouped navigation, a queue column and full-width playback controls.
- Play upcoming tracks directly from Home, including permission-aware remote queue selection.
- Add native Android Google authorization and paginated read-only YouTube playlist access, with native-only tokens, revocation and OAuth setup instructions.
- Refresh README screenshots and document release setup, checks and remaining Android playback limitations.

## 0.4.0 — 2026-10-20

- Add the Windows virtual microphone preview, shared playback controls and experimental Android arm64 APK.
- Publish five desktop installer formats and the Android evaluation APK with verified checksums.

## 0.3.3 — 2026-10-18

- Fix Linux builds by pinning the INI parser version required by Tauri's deep-link plugin.
- Publish versioned releases automatically after web and desktop checks, all five installer builds, and checksum verification succeed.
- Add a packaging test mode that saves installers as workflow artifacts without publishing a release.
- Read the app's displayed version from its package manifest so About stays in sync with the installer.
- Refresh the opening README screenshot with electronic playlists and document a proposed Windows virtual microphone integration.

The virtual microphone remains a proposal; audio capture and mixing are not included in this release. macOS and Linux installer builds still require native behavior testing.
