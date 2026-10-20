# Virtual microphone

YTM Listen can route the local Windows player's music to a virtual audio input, with an optional physical microphone mixed into the same signal. Open **Virtual microphone** in the desktop sidebar.

This first version requires Windows 11 or Windows build 20348 or newer and an installed virtual audio cable. It does not create its own Windows audio driver. macOS, Linux, and Android audio routing are not implemented.

## Setup

1. Install [VB-CABLE from VB-Audio](https://vb-audio.com/Cable/) if you do not already have a compatible cable. Follow the vendor's instructions, including a reboot if requested. VB-CABLE is donationware and retains the vendor's device names.
2. Open YTM Listen's **Virtual microphone** page and select **Refresh devices**.
3. Select **CABLE Input (VB-Audio Virtual Cable)** as the virtual output. Only recognized virtual cable render endpoints are offered, so ordinary speakers cannot be selected accidentally.
4. Start a song on this computer, then select **Start sharing**. Local playback continues normally.
5. In Discord, OBS, or another app, select **CABLE Output (VB-Audio Virtual Cable)** as its microphone/input device. The cable's Input is its playback endpoint; Output is its recording endpoint.
6. To include your voice, choose your physical microphone and enable **Include microphone** separately. Use its gain and mute controls to balance the mix. A live microphone is not monitored through your speakers.

The app does not change Windows' default input or output devices. Cable names can differ for additional VB-Audio cables or Voicemeeter endpoints; choose the corresponding recording endpoint in the receiving application.

## Controls and quality

- Music sharing and microphone inclusion both start off after an app restart. Device choices and levels are remembered separately.
- Shared music volume and microphone gain control their contributions to the mix. The existing player volume and mute can also affect captured music; independent headphone volume is not promised.
- Meters show music, microphone, and combined output levels. Stereo-linked peak protection prevents the summed output from clipping. Lower the source levels if peak protection is frequently active.
- The mixer uses 48 kHz stereo, 32-bit floating-point PCM. Windows converts physical microphone and cable formats where necessary. A mono microphone is sent to both channels.
- YTM Listen adds no extra lossy music encoding. Source quality still depends on the stream available to your account, and the receiving application can apply compression, downmixing, and noise suppression.
- Use headphones to prevent your physical microphone from acoustically picking up music or incoming calls. The app captures only the playback WebView process tree, not all system audio.

A sharing banner stays available when you navigate to another page. It includes Stop and, when enabled, microphone mute. The desktop tray also shows whether music and microphone sharing are active and provides **Mute shared microphone** and **Stop audio sharing** while the window is hidden. Stopping releases the capture and output streams. Muting or disabling the microphone closes its capture client and flushes queued mixed audio before acknowledging the change. Audio already consumed or buffered by another application cannot be recalled.

## Interruptions

Disconnecting the microphone disables its contribution while music can continue. The app never switches to a different microphone automatically. Losing the selected virtual output stops sharing. After sleep, a device stall, or a playback-browser restart, sharing may stop and require you to start it again. Re-enable the microphone explicitly when needed.

If a device is missing, reconnect it, refresh devices, and select it again. If Windows denies microphone access, enable access for desktop applications in Windows settings before trying again. If music is audible locally but absent from the shared input, check player mute/volume, cable endpoint selection, and the receiving application's input meter.

For Discord, test with noise suppression disabled and an appropriate voice-channel bitrate. See Discord's [noise suppression FAQ](https://support.discord.com/hc/en-us/articles/360040843952-Krisp-FAQ) and [bitrate guide](https://support.discord.com/hc/en-us/articles/11635925354775-Audio-Bitrate-FAQ).

## Verification

The Windows development capture check passed on 2026-10-20 using the actual playback WebView: a synthetic playback tone was captured (peak approximately 0.02), a simultaneous native-app tone was excluded (peak 0), and capture returned to silence afterward. The check did not open a microphone or save audio.

Automated tests cover explicit mic opt-in, mute and stop UI behavior, an active indicator outside the audio page, input validation, clearing stale samples, limiting, bounded buffers, and five minutes of simulated independent clock drift. Native code passes the desktop compiler and Clippy checks.

No supported virtual cable was installed on the development PC during this implementation. Full cable-to-Discord/OBS playback, physical microphone mixing, hardware unplug/replug, real long-session drift, and measured end-to-end latency still require testing with that driver and the receiving app. The Premium playback path uses the same process capture, but this synthetic capture check alone does not prove capture of every protected stream.

Developers can run `ytm-listen --profile audio-probe --check-audio-routing` for the synthetic capture check. It plays two short, quiet test tones, uses an isolated profile, emits a JSON result, and exits. It never records microphone audio.
