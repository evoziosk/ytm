# Contributing

Install the prerequisites and follow the README. Keep the remote-page boundary
small: only `player_event` and `catalog_result` are granted to the playback service at music.youtube.com. Catalog replies must match a pending random request ID and pass metadata validation. Every application
command must be listed in `src-tauri/build.rs` so Tauri's command ACL applies.
Never grant remote content filesystem, shell, arbitrary invoke, or window APIs.

Run `pnpm check`, `pnpm test`, `pnpm build`, `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace`.
If changing protocol types, run `pnpm types:generate` and commit the generated
TypeScript. CI checks for drift. Do not hand-edit generated.ts.

For sync changes, test host/member play, pause, seek, track replacement, ads,
buffering, network interruption, host transfer, and late joins. Use development
profiles to keep accounts and storage separate. Keep credentials out of logs.
Test relay changes in the real Workers runtime, not just a WebSocket mock.

## When YouTube changes its page

1. Check whether `#movie_player.getVideoData()` and the `<video>` element still
   work. Prefer these and Media Session metadata over page text.
2. Update fragile selectors in `apps/desktop/bridge/src/selectors.ts` only.
3. Confirm likes do not become dislikes, ad detection clears at ad end, and a
   replaced video element gets fresh event listeners.
4. Run the bridge tests and rebuild it. Rust embeds its built IIFE at compile time.
5. Use a disposable development profile for manual checks. Do not post cookies,
   authorization headers, or account details in an issue.

## Releases

Update the workspace Cargo version, `tauri.conf.json`, and package.json versions
together. Regenerate both lockfiles, run checks, then push a matching `vX.Y.Z`
tag to your own repository. The release workflow runs the complete CI checks,
builds all five Windows/macOS/Linux installer formats, includes the experimental
Android arm64 test APK, and verifies checksums. Add version-specific release notes
in `docs/releases/vX.Y.Z.md` when preparing a version.
Only after every job passes does it publish the complete release automatically.
Normal `main` pushes do not publish releases, and published versions cannot be
overwritten. To test the installer builds before tagging, manually run the
Release workflow with `dry_run` enabled (the default); packages are saved as
workflow artifacts and no release is created. Manual publishing requires
running the workflow against a matching stable version tag with `dry_run` off.
Signing/notarization and a signed updater are intentionally not configured with
placeholder credentials. A maintainer must supply their own signing identities
and update hosting before enabling those features.
