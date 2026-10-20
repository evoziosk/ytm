//! All WASAPI interfaces live and are released on one dedicated MTA thread.
use super::{
    dsp::{ClockBuffer, Mixer, RATE},
    Device, Devices, Routing, Status,
};
use std::{
    mem::{size_of, ManuallyDrop},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};
use windows::{
    core::{implement, IUnknown, Interface, Ref, HRESULT},
    Wdk::System::SystemServices::RtlGetVersion,
    Win32::{
        Devices::FunctionDiscovery::PKEY_Device_FriendlyName,
        Media::Audio::*,
        System::{
            Com::{
                CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize,
                StructuredStorage::{
                    PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
                },
                BLOB, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
            },
            SystemInformation::OSVERSIONINFOW,
            Variant::VT_BLOB,
        },
    },
};

type Result<T> = std::result::Result<T, String>;
fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}
struct Apartment;
impl Apartment {
    fn new() -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED)
                .ok()
                .map_err(err)?;
        }
        Ok(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

pub fn supported() -> bool {
    let mut version = OSVERSIONINFOW {
        dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    unsafe { RtlGetVersion(&mut version).is_ok() && version.dwBuildNumber >= 20348 }
}
pub async fn playback_pid(app: &AppHandle) -> Result<u32> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.get_webview_window("main")
        .ok_or("The local playback window is unavailable")?
        .with_webview(move |view| {
            let value = (|| unsafe {
                let mut pid = 0;
                view.controller()
                    .CoreWebView2()?
                    .BrowserProcessId(&mut pid)?;
                Ok::<_, windows::core::Error>(pid)
            })()
            .map_err(err);
            let _ = sender.send(value);
        })
        .map_err(err)?;
    let pid = tokio::time::timeout(Duration::from_secs(3), receiver)
        .await
        .map_err(|_| "Playback process lookup timed out")?
        .map_err(err)??;
    if pid == 0 || pid == std::process::id() {
        return Err("Cannot safely isolate the music process".into());
    }
    Ok(pid)
}

fn enumerator() -> Result<IMMDeviceEnumerator> {
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(err) }
}
fn id(device: &IMMDevice) -> Result<String> {
    unsafe {
        let pointer = device.GetId().map_err(err)?;
        let value = pointer.to_string().map_err(err);
        CoTaskMemFree(Some(pointer.0.cast()));
        value
    }
}
fn name(device: &IMMDevice) -> Result<String> {
    unsafe {
        let store = device.OpenPropertyStore(STGM_READ).map_err(err)?;
        let value = store.GetValue(&PKEY_Device_FriendlyName).map_err(err)?;
        let text = windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc(&value)
            .map_err(err)?;
        let result = text.to_string().map_err(err);
        CoTaskMemFree(Some(text.0.cast()));
        result
    }
}
fn virtual_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.contains("vb-audio")
        || name.contains("vb-cable")
        || name.contains("cable input")
        || name.contains("cable output")
        || name.contains("cable-a ")
        || name.contains("cable-b ")
        || name.contains("cable-c ")
        || name.contains("cable-d ")
        || name.contains("voicemeeter")
}
fn device_list(enumerator: &IMMDeviceEnumerator, direction: EDataFlow) -> Result<Vec<Device>> {
    unsafe {
        let devices = enumerator
            .EnumAudioEndpoints(direction, DEVICE_STATE_ACTIVE)
            .map_err(err)?;
        let mut list = Vec::new();
        for index in 0..devices.GetCount().map_err(err)? {
            let device = devices.Item(index).map_err(err)?;
            let name = name(&device)?;
            list.push(Device {
                id: id(&device)?,
                virtual_cable: virtual_name(&name),
                name,
            });
        }
        list.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(list)
    }
}
pub fn devices() -> Result<Devices> {
    let _com = Apartment::new()?;
    let enumerator = enumerator()?;
    Ok(Devices {
        outputs: device_list(&enumerator, eRender)?
            .into_iter()
            .filter(|device| device.virtual_cable)
            .collect(),
        microphones: device_list(&enumerator, eCapture)?
            .into_iter()
            .filter(|device| !device.virtual_cable)
            .collect(),
    })
}
fn select(
    enumerator: &IMMDeviceEnumerator,
    selected: &str,
    direction: EDataFlow,
    cable: bool,
) -> Result<IMMDevice> {
    if !device_list(enumerator, direction)?
        .iter()
        .any(|device| device.id == selected && device.virtual_cable == cable)
    {
        return Err(if cable {
            "Select an installed virtual cable playback device"
        } else {
            "Select a connected physical microphone"
        }
        .into());
    }
    unsafe {
        enumerator
            .GetDevice(&windows::core::HSTRING::from(selected))
            .map_err(err)
    }
}

#[implement(IActivateAudioInterfaceCompletionHandler)]
struct Completion(mpsc::SyncSender<()>);
impl IActivateAudioInterfaceCompletionHandler_Impl for Completion_Impl {
    fn ActivateCompleted(
        &self,
        _: Ref<IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        let _ = self.0.try_send(());
        Ok(())
    }
}
fn process_client(pid: u32) -> Result<IAudioClient> {
    unsafe {
        let mut params = AUDIOCLIENT_ACTIVATION_PARAMS {
            ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
            Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
                ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                    TargetProcessId: pid,
                    ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
                },
            },
        };
        // The blob borrows params; PROPVARIANT must not free this stack memory.
        let prop = ManuallyDrop::new(PROPVARIANT {
            Anonymous: PROPVARIANT_0 {
                Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 {
                    vt: VT_BLOB,
                    Anonymous: PROPVARIANT_0_0_0 {
                        blob: BLOB {
                            cbSize: size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
                            pBlobData: std::ptr::from_mut(&mut params).cast(),
                        },
                    },
                    ..Default::default()
                }),
            },
        });
        let (sender, receiver) = mpsc::sync_channel(1);
        let callback: IActivateAudioInterfaceCompletionHandler = Completion(sender).into();
        let operation = ActivateAudioInterfaceAsync(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &IAudioClient::IID,
            Some(&*prop),
            &callback,
        )
        .map_err(err)?;
        // Activation copies parameters before returning. The COM callback owns
        // its sender until completion, even if our bounded wait times out.
        receiver
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "Windows did not activate music capture in time")?;
        let mut result = HRESULT(0);
        let mut object: Option<IUnknown> = None;
        operation
            .GetActivateResult(&mut result, &mut object)
            .map_err(err)?;
        result.ok().map_err(err)?;
        object
            .ok_or("Windows returned no audio interface")?
            .cast()
            .map_err(err)
    }
}
fn format(channels: u16) -> WAVEFORMATEX {
    // IEEE float, 48 kHz. Windows' high-quality shared-mode converter handles
    // the physical endpoint's native sample rate and bit depth.
    WAVEFORMATEX {
        wFormatTag: 3,
        nChannels: channels,
        nSamplesPerSec: RATE as u32,
        nAvgBytesPerSec: RATE as u32 * channels as u32 * 4,
        nBlockAlign: channels * 4,
        wBitsPerSample: 32,
        cbSize: 0,
    }
}
fn initialize(client: &IAudioClient, channels: u16, loopback: bool) -> Result<()> {
    let flags = AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
        | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY
        | if loopback {
            AUDCLNT_STREAMFLAGS_LOOPBACK
        } else {
            0
        };
    unsafe {
        client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                flags,
                200_000,
                0,
                &format(channels),
                None,
            )
            .map_err(err)
    }
}
struct Stream(IAudioClient);
impl Drop for Stream {
    fn drop(&mut self) {
        unsafe {
            let _ = self.0.Stop();
            let _ = self.0.Reset();
        }
    }
}
struct Capture {
    _stream: Stream,
    capture: IAudioCaptureClient,
    channels: usize,
    buffer: ClockBuffer,
}
impl Capture {
    fn new(client: IAudioClient, channels: u16, loopback: bool) -> Result<Self> {
        initialize(&client, channels, loopback)?;
        let stream = Stream(client);
        let capture = unsafe { stream.0.GetService().map_err(err)? };
        unsafe {
            stream.0.Start().map_err(err)?;
        }
        Ok(Self {
            _stream: stream,
            capture,
            channels: channels as usize,
            buffer: ClockBuffer::new(),
        })
    }
    fn microphone(enumerator: &IMMDeviceEnumerator, selected: &str) -> Result<Self> {
        let device = select(enumerator, selected, eCapture, false)?;
        let client: IAudioClient = unsafe { device.Activate(CLSCTX_ALL, None).map_err(err)? };
        let mix = unsafe { client.GetMixFormat().map_err(err)? };
        let channels = unsafe {
            let channels = (*mix).nChannels;
            CoTaskMemFree(Some(mix.cast()));
            channels
        };
        // Request mono for mono devices and stereo otherwise. Never turn a
        // rejected input format into capture of a different/default device.
        Self::new(client, channels.clamp(1, 2), false)
    }
    fn drain(&mut self, discard: bool) -> Result<()> {
        unsafe {
            for _ in 0..64 {
                if self.capture.GetNextPacketSize().map_err(err)? == 0 {
                    break;
                }
                let mut pointer = std::ptr::null_mut();
                let mut frames = 0;
                let mut flags = 0;
                self.capture
                    .GetBuffer(&mut pointer, &mut frames, &mut flags, None, None)
                    .map_err(err)?;
                // WASAPI may supply a null pointer with SILENT. Never make a
                // slice from it. Every acquired buffer is released once.
                let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || pointer.is_null();
                if flags & AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY.0 as u32 != 0 {
                    self.buffer.clear();
                }
                if !discard {
                    for frame in 0..frames as usize {
                        let mut sample = [0.0; 2];
                        if !silent {
                            for (channel, value) in sample.iter_mut().enumerate() {
                                let index = frame * self.channels + channel.min(self.channels - 1);
                                *value =
                                    std::ptr::read_unaligned(pointer.add(index * 4).cast::<f32>());
                            }
                        }
                        self.buffer.push(sample);
                    }
                }
                self.capture.ReleaseBuffer(frames).map_err(err)?;
            }
        }
        if discard {
            self.buffer.clear();
        }
        Ok(())
    }
}
struct Output {
    stream: Stream,
    render: IAudioRenderClient,
    size: u32,
}
impl Output {
    fn new(enumerator: &IMMDeviceEnumerator, selected: &str) -> Result<Self> {
        let device = select(enumerator, selected, eRender, true)?;
        let client = unsafe { device.Activate(CLSCTX_ALL, None).map_err(err)? };
        initialize(&client, 2, false)?;
        let stream = Stream(client);
        let render = unsafe { stream.0.GetService().map_err(err)? };
        let size = unsafe { stream.0.GetBufferSize().map_err(err)? };
        unsafe {
            stream.0.Start().map_err(err)?;
        }
        Ok(Self {
            stream,
            render,
            size,
        })
    }
    fn flush(&self) -> Result<()> {
        unsafe {
            self.stream.0.Stop().map_err(err)?;
            self.stream.0.Reset().map_err(err)?;
            self.stream.0.Start().map_err(err)
        }
    }
    fn render(
        &self,
        music: &mut Capture,
        mic: &mut Option<Capture>,
        routing: &Routing,
        mixer: &mut Mixer,
    ) -> Result<(f32, f32, f32, bool)> {
        unsafe {
            let frames = self
                .size
                .saturating_sub(self.stream.0.GetCurrentPadding().map_err(err)?);
            if frames == 0 {
                return Ok((0.0, 0.0, 0.0, false));
            }
            let pointer = self.render.GetBuffer(frames).map_err(err)?;
            let mut music_peak: f32 = 0.0;
            let mut mic_peak: f32 = 0.0;
            let mut output_peak: f32 = 0.0;
            let mut limited = false;
            for frame in 0..frames as usize {
                let music_sample = music.buffer.next();
                let mic_sample = if routing.microphone_enabled && !routing.microphone_muted {
                    mic.as_mut()
                        .map(|source| source.buffer.next())
                        .unwrap_or([0.0; 2])
                } else {
                    [0.0; 2]
                };
                let (sample, clipped) = mixer.mix(
                    music_sample,
                    mic_sample,
                    routing.preferences.music_gain,
                    routing.preferences.microphone_gain,
                );
                limited |= clipped;
                for channel in 0..2 {
                    music_peak = music_peak
                        .max((music_sample[channel] * routing.preferences.music_gain).abs());
                    mic_peak = mic_peak
                        .max((mic_sample[channel] * routing.preferences.microphone_gain).abs());
                    output_peak = output_peak.max(sample[channel].abs());
                    std::ptr::write_unaligned(
                        pointer.add((frame * 2 + channel) * 4).cast::<f32>(),
                        sample[channel],
                    );
                }
            }
            self.render.ReleaseBuffer(frames, 0).map_err(err)?;
            Ok((music_peak, mic_peak, output_peak, limited))
        }
    }
}

struct Update {
    routing: Routing,
    reply: mpsc::SyncSender<Result<()>>,
}
pub struct Worker {
    pub pid: u32,
    stop: Arc<AtomicBool>,
    updates: mpsc::SyncSender<Update>,
    thread: thread::JoinHandle<()>,
}
impl Worker {
    pub fn start(pid: u32, routing: Routing, status: Arc<Mutex<Status>>) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let halt = stop.clone();
        let (updates, receiver) = mpsc::sync_channel(4);
        let (ready, startup) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("ytm-audio-mix".into())
            .spawn(move || {
                let result = run(pid, routing, status.clone(), &halt, receiver, ready.clone());
                if let Err(error) = result {
                    let _ = ready.try_send(Err(error.clone()));
                    let mut state = status.lock().expect("audio status");
                    state.message = error;
                }
                let mut state = status.lock().expect("audio status");
                state.active = false;
                state.microphone_enabled = false;
                state.microphone_muted = false;
                state.music_peak = 0.0;
                state.microphone_peak = 0.0;
                state.output_peak = 0.0;
                state.buffered_ms = 0;
            })
            .map_err(err)?;
        match startup.recv_timeout(Duration::from_secs(8)) {
            Ok(Ok(())) => Ok(Self {
                pid,
                stop,
                updates,
                thread,
            }),
            Ok(Err(error)) => {
                stop.store(true, Ordering::Release);
                let _ = thread.join();
                Err(error)
            }
            Err(error) => {
                stop.store(true, Ordering::Release);
                let _ = thread.join();
                Err(err(error))
            }
        }
    }
    pub fn apply(&self, routing: Routing) -> Result<()> {
        let (reply, response) = mpsc::sync_channel(1);
        self.updates
            .try_send(Update { routing, reply })
            .map_err(|_| "Audio worker is busy or stopped")?;
        match response.recv_timeout(Duration::from_secs(4)) {
            Ok(result) => result,
            Err(_) => {
                self.stop.store(true, Ordering::Release);
                Err(
                    "Audio change was not acknowledged. Sharing is stopping; retry after it stops."
                        .into(),
                )
            }
        }
    }
    pub fn stop(self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.thread.join();
    }
}

fn run(
    pid: u32,
    mut routing: Routing,
    status: Arc<Mutex<Status>>,
    stop: &AtomicBool,
    updates: mpsc::Receiver<Update>,
    ready: mpsc::SyncSender<Result<()>>,
) -> Result<()> {
    let _com = Apartment::new()?;
    let enumerator = enumerator()?;
    let output = Output::new(&enumerator, &routing.preferences.output_id)?;
    let mut music = Capture::new(process_client(pid)?, 2, true)
        .map_err(|error| format!("Music capture unavailable: {error}"))?;
    let mut mic: Option<Capture> = None;
    let mut mixer = Mixer::new();
    let mut last_meter = Instant::now();
    let mut last_tick = Instant::now();
    let mut last_device = Instant::now();
    let mut peaks = (0.0_f32, 0.0_f32, 0.0_f32, false);
    {
        let mut state = status.lock().expect("audio status");
        *state = Status {
            active: true,
            supported: true,
            preferences: routing.preferences.clone(),
            message: "Sharing music · 48 kHz stereo PCM".into(),
            ..Default::default()
        };
    }
    let _ = ready.send(Ok(()));
    while !stop.load(Ordering::Acquire) {
        if last_tick.elapsed() > Duration::from_secs(2) {
            return Err(
                "Audio sharing stopped after sleep or a device stall. Start sharing again.".into(),
            );
        }
        last_tick = Instant::now();
        while let Ok(update) = updates.try_recv() {
            let next = update.routing;
            if super::needs_microphone_confirmation(&routing, &next) {
                let _ = update.reply.send(Err(
                    "Microphone state changed. Enable or unmute it explicitly to share your voice."
                        .into(),
                ));
                continue;
            }
            if next.preferences.output_id != routing.preferences.output_id {
                let _ = update
                    .reply
                    .send(Err("Stop sharing before changing the virtual output".into()));
                continue;
            }
            let mic_changed = next.microphone_enabled != routing.microphone_enabled
                || next.microphone_muted != routing.microphone_muted
                || next.preferences.microphone_id != routing.preferences.microphone_id;
            if mic_changed {
                // Drop the old capture first. Clear music and output too, so
                // buffers already containing voice cannot leak after the ack.
                mic = None;
                routing.microphone_enabled = false;
                routing.microphone_muted = true;
                music.buffer.clear();
                output.flush()?;
                peaks = (0.0, 0.0, 0.0, false);
                if next.microphone_enabled && !next.microphone_muted {
                    match Capture::microphone(&enumerator, &next.preferences.microphone_id) {
                        Ok(source) => mic = Some(source),
                        Err(error) => {
                            let mut state = status.lock().expect("audio status");
                            state.microphone_enabled = false;
                            state.microphone_muted = true;
                            state.microphone_peak = 0.0;
                            state.output_peak = 0.0;
                            state.message = format!("Microphone stopped: {error}");
                            let _ = update.reply.send(Err(state.message.clone()));
                            continue;
                        }
                    }
                }
            }
            routing = next;
            {
                let mut state = status.lock().expect("audio status");
                state.preferences = routing.preferences.clone();
                state.microphone_enabled = routing.microphone_enabled;
                state.microphone_muted = routing.microphone_muted;
                if mic_changed {
                    state.music_peak = 0.0;
                    state.microphone_peak = 0.0;
                    state.output_peak = 0.0;
                    state.limited = false;
                }
                state.message = "Sharing music · 48 kHz stereo PCM".into();
            }
            let _ = update.reply.send(Ok(()));
        }
        music
            .drain(false)
            .map_err(|error| format!("Music capture stopped: {error}"))?;
        if let Some(source) = mic.as_mut() {
            if let Err(error) = source.drain(false) {
                mic = None;
                routing.microphone_enabled = false;
                routing.microphone_muted = true;
                music.buffer.clear();
                output.flush()?;
                peaks = (0.0, 0.0, 0.0, false);
                let mut state = status.lock().expect("audio status");
                state.microphone_enabled = false;
                state.microphone_muted = true;
                state.message = format!("Microphone disconnected. Music continues. {error}");
            }
        }
        if last_device.elapsed() > Duration::from_secs(1) {
            select(&enumerator, &routing.preferences.output_id, eRender, true)
                .map_err(|_| "Virtual output disconnected. Sharing stopped.")?;
            if mic.is_some()
                && select(
                    &enumerator,
                    &routing.preferences.microphone_id,
                    eCapture,
                    false,
                )
                .is_err()
            {
                mic = None;
                routing.microphone_enabled = false;
                routing.microphone_muted = true;
                music.buffer.clear();
                output.flush()?;
                let mut state = status.lock().expect("audio status");
                state.microphone_enabled = false;
                state.microphone_muted = true;
                state.message = "Microphone disconnected. Music continues.".into();
            }
            last_device = Instant::now();
        }
        let values = output
            .render(&mut music, &mut mic, &routing, &mut mixer)
            .map_err(|error| format!("Virtual output stopped: {error}"))?;
        peaks.0 = peaks.0.max(values.0);
        peaks.1 = peaks.1.max(values.1);
        peaks.2 = peaks.2.max(values.2);
        peaks.3 |= values.3;
        if last_meter.elapsed() >= Duration::from_millis(100) {
            let mut state = status.lock().expect("audio status");
            state.music_peak = peaks.0;
            state.microphone_peak = peaks.1;
            state.output_peak = peaks.2;
            state.limited = peaks.3;
            state.underruns =
                music.buffer.underruns + mic.as_ref().map_or(0, |m| m.buffer.underruns);
            state.dropped_frames =
                music.buffer.dropped + mic.as_ref().map_or(0, |m| m.buffer.dropped);
            state.buffered_ms = (music.buffer.len() * 1000 / RATE) as u32;
            peaks = (0.0, 0.0, 0.0, false);
            last_meter = Instant::now();
        }
        thread::sleep(Duration::from_millis(3));
    }
    Ok(())
}

/// Capture proof uses a quiet native tone (which must be excluded), then a
/// tone from the actual playback WebView (which must be included).
pub fn probe(
    pid: u32,
    ready: tokio::sync::oneshot::Sender<Result<()>>,
) -> Result<serde_json::Value> {
    let _com = Apartment::new()?;
    let enumerator = enumerator()?;
    let outputs = device_list(&enumerator, eRender)?;
    let microphones = device_list(&enumerator, eCapture)?;
    let mut capture = match Capture::new(process_client(pid)?, 2, true) {
        Ok(capture) => capture,
        Err(error) => {
            let _ = ready.send(Err(error.clone()));
            return Err(error);
        }
    };
    let device = unsafe {
        enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .map_err(err)?
    };
    let client: IAudioClient = unsafe { device.Activate(CLSCTX_ALL, None).map_err(err)? };
    initialize(&client, 2, false)?;
    let native = Stream(client);
    let render: IAudioRenderClient = unsafe { native.0.GetService().map_err(err)? };
    let size = unsafe { native.0.GetBufferSize().map_err(err)? };
    unsafe {
        native.0.Start().map_err(err)?;
    }
    let _ = ready.send(Ok(()));
    let started = Instant::now();
    let mut tone_frame = 0;
    let mut excluded_peak = 0.0_f32;
    let mut music_peak = 0.0_f32;
    let mut silence_peak = 0.0_f32;
    while started.elapsed() < Duration::from_secs(4) {
        let elapsed = started.elapsed().as_secs_f64();
        unsafe {
            let frames = size.saturating_sub(native.0.GetCurrentPadding().map_err(err)?);
            if frames > 0 {
                let data = render.GetBuffer(frames).map_err(err)?;
                for frame in 0..frames as usize {
                    let value = if elapsed < 1.0 {
                        (tone_frame as f32 * 440.0 * std::f32::consts::TAU / RATE as f32).sin()
                            * 0.02
                    } else {
                        0.0
                    };
                    for channel in 0..2 {
                        std::ptr::write_unaligned(
                            data.add((frame * 2 + channel) * 4).cast::<f32>(),
                            value,
                        );
                    }
                    tone_frame += 1;
                }
                render.ReleaseBuffer(frames, 0).map_err(err)?;
            }
        }
        capture.drain(false)?;
        // Inspect raw captured samples, without the mixer's startup buffering.
        let peak = capture.buffer.peak();
        if (0.2..1.0).contains(&elapsed) {
            excluded_peak = excluded_peak.max(peak);
        }
        if (1.9..2.7).contains(&elapsed) {
            music_peak = music_peak.max(peak);
        }
        if elapsed > 3.7 {
            silence_peak = silence_peak.max(peak);
        }
        capture.buffer.clear();
        thread::sleep(Duration::from_millis(5));
    }
    let passed = excluded_peak < 0.0001 && music_peak > 0.001 && silence_peak < 0.0001;
    Ok(
        serde_json::json!({ "passed": passed, "native_audio_excluded_peak": excluded_peak, "playback_webview_peak": music_peak,
        "after_tone_peak": silence_peak, "outputs": outputs, "microphones": microphones, "microphone_opened": false,
        "sample_rate": RATE, "channels": 2 }),
    )
}
