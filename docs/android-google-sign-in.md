# Android Google library setup

Android now uses Google Play services' native authorization screen to connect a
Google account for **read-only YouTube playlist access**. It does not send Google
sign-in through the playback WebView. Account selection, consent and token caching
are handled by Google Play services. Tokens are used only in Kotlin for requests to
Google's fixed API endpoints; neither the Svelte interface nor the playback website
receives them. App preferences store only the selected account's email, display
name and connection choice.

This is an experimental account/library implementation. Google Cloud registration
and physical-device acceptance are required. It does **not** authenticate the
YouTube Music playback WebView or enable Premium playback. The native library
currently lists your own YouTube playlists, opens their tracks and supports
pagination. It does not claim parity with the full Music library, saved albums,
uploads or Music recommendations. Search and playback continue using their
existing session.

## Configure Google Cloud

1. Open [Google Cloud Console](https://console.cloud.google.com/) and create a
   project for YTM Listen.
2. Under **APIs & Services → Library**, enable **YouTube Data API v3**.
3. Configure Google Auth Platform's **Branding**, **Audience** and **Data Access**.
   For a personal test, choose an external app in testing and add the Google
   account you will use to the test-user list. Complete the contact information
   and any homepage/privacy fields Google requires.
4. Add these scopes:
   - `https://www.googleapis.com/auth/youtube.readonly`
   - `https://www.googleapis.com/auth/userinfo.email`
   - `https://www.googleapis.com/auth/userinfo.profile`
5. Under **Clients**, create an **Android** OAuth client with package name
   `io.ytmlisten.desktop` and the **SHA-1 certificate fingerprint of the APK you
   will install**. The APK's SHA-256 file checksum is a different value and cannot
   be used here.
6. Install that APK on a device with current Google Play services. Open
   **Account → Account settings → Connect Google library**, choose the account
   and grant library/profile access. Open **Your library** to browse playlists.

Google identifies the Android OAuth client using the app's package name and signing
certificate. There is no client secret to paste into this app. Do not create a TV
or desktop OAuth client for this Android flow. Debug/test and maintainer-signed
APKs need separate Android client registrations when their signing keys differ.

The native connection error dialog includes **Copy setup details**, which copies
the actual installed package, signing SHA-1 and requested scopes. A local APK build
can also be inspected with Android SDK `apksigner`:

```sh
apksigner verify --print-certs path/to/ytm-listen.apk
```

Use **Signer #1 certificate SHA-1 digest** from that output. Keep the test signing
key stable between local APK builds if you want the same OAuth registration to
continue working. The local test APK's companion `android-google-oauth-setup.txt`
contains its concrete fingerprint and checksum.

## Reconnect and disconnect

The app restores a previously authorized library connection silently on launch
when Google can provide a current token without consent. If access expires, is
revoked or needs renewed consent, it asks you to reconnect from Account; browsing
does not launch a consent prompt unexpectedly. **Disconnect Google** clears the
local account choice and requests revocation of the app's Google grants. If
revocation cannot reach Google, the app reports that it disconnected locally and
asks you to remove the grant in your Google Account's third-party connections.

## Acceptance checks

- Register the actual installed APK fingerprint, connect a consent-screen test
  user, and confirm that Google authorization completes outside the WebView.
- Browse a private playlist, load its next page and play a listed track. Confirm
  that duplicate playlist entries retain distinct queue identities.
- Restart the app and verify silent library restoration. Revoke access from the
  Google Account and verify reconnection is requested without exposing stale
  private results.
- Cancel consent, deny library access, test missing/outdated Play services, lose
  connectivity, and try an APK whose certificate is not registered. Check the
  actionable setup error and its copied fingerprint.
- Disconnect while a library query is running and verify that its result is
  discarded. Verify guest playback events cannot overwrite native account status.
- Continue the device playback, background service and remote-control checks in
  [shared-control.md](shared-control.md#android-feasibility-build). Successful
  library authorization is not evidence of authenticated Music playback.

References: [Android Google authorization](https://developer.android.com/identity/authorization),
[YouTube playlist API](https://developers.google.com/youtube/v3/docs/playlists/list),
[playlist entries](https://developers.google.com/youtube/v3/docs/playlistItems/list).
