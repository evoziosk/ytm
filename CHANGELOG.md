# Changelog

## 0.3.3 — 2026-10-18

- Fix Linux builds by pinning the INI parser version required by Tauri's deep-link plugin.
- Publish versioned releases automatically after web and desktop checks, all five installer builds, and checksum verification succeed.
- Add a packaging test mode that saves installers as workflow artifacts without publishing a release.
- Read the app's displayed version from its package manifest so About stays in sync with the installer.
- Refresh the opening README screenshot with electronic playlists and document a proposed Windows virtual microphone integration.

The virtual microphone remains a proposal; audio capture and mixing are not included in this release. macOS and Linux installer builds still require native behavior testing.
