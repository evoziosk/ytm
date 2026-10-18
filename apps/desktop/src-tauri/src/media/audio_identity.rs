//! WebView2 owns the audio stream, so the mixer otherwise inherits Edge's icon.
//! Scope changes to processes from the playback WebView's own environment.
use std::{os::windows::ffi::OsStrExt, path::PathBuf, time::Duration};
use tauri::{AppHandle, Manager};
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment8;
use windows::{
    core::{w, Interface, PCWSTR, PWSTR},
    Win32::{
        Media::Audio::{
            eRender, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator,
            MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
        },
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
            COINIT_MULTITHREADED,
        },
    },
};

const ICON: &[u8] = include_bytes!("../../icons/icon.ico");
const NAME: &str = "YTM Listen";

pub struct SessionIdentity {
    pub name: String,
    pub icon_matches: bool,
}

fn icon_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("ytm-listen-audio.ico"))
}

pub fn start(app: &AppHandle) -> Result<(), String> {
    let path = icon_path(app)?;
    std::fs::create_dir_all(path.parent().ok_or("Icon directory unavailable")?)
        .map_err(|error| error.to_string())?;
    if std::fs::read(&path).ok().as_deref() != Some(ICON) {
        std::fs::write(&path, ICON).map_err(|error| error.to_string())?;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            if let Err(error) = refresh(&app, path.clone(), true).await {
                // No audio device, a disconnected endpoint, or a restarting
                // WebView must not stop playback or prevent a later retry.
                tracing::debug!(%error, "Windows mixer identity update deferred");
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
    Ok(())
}

/// Read back the real Windows session metadata for explicit development checks.
pub async fn inspect(app: &AppHandle) -> Result<Vec<SessionIdentity>, String> {
    refresh(app, icon_path(app)?, false).await
}

async fn refresh(
    app: &AppHandle,
    path: PathBuf,
    apply: bool,
) -> Result<Vec<SessionIdentity>, String> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.get_webview_window("main")
        .ok_or("Playback window unavailable")?
        .with_webview(move |webview| {
            // WebView2 COM interfaces stay on their UI thread. Only process
            // IDs cross to the worker that enumerates Windows audio sessions.
            let result = (|| unsafe {
                let environment: ICoreWebView2Environment8 = webview.environment().cast()?;
                let processes = environment.GetProcessInfos()?;
                let mut count = 0;
                processes.Count(&mut count)?;
                let mut ids = Vec::new();
                for index in 0..count {
                    let mut id = 0;
                    processes.GetValueAtIndex(index)?.ProcessId(&mut id)?;
                    if id > 0 {
                        ids.push(id as u32);
                    }
                }
                Ok::<_, windows::core::Error>(ids)
            })();
            let _ = sender.send(result.map_err(|error| error.to_string()));
        })
        .map_err(|error| error.to_string())?;
    let ids = receiver.await.map_err(|error| error.to_string())??;
    tokio::task::spawn_blocking(move || sessions(&ids, &path, apply))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

struct ComApartment;
impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

// Both getters allocate with COM's task allocator, including empty strings.
unsafe fn take_string(value: PWSTR) -> String {
    let text = if value.is_null() {
        String::new()
    } else {
        value.to_string().unwrap_or_default()
    };
    CoTaskMemFree(Some(value.0.cast()));
    text
}

fn sessions(
    ids: &[u32],
    path: &std::path::Path,
    apply: bool,
) -> windows::core::Result<Vec<SessionIdentity>> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        let _apartment = ComApartment;
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        // Include all output devices, not just the default: users can route
        // this app to headphones or change devices while it is running.
        let devices = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
        let path_wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let expected_path = path.to_string_lossy();
        let mut found = Vec::new();
        for device_index in 0..devices.GetCount()? {
            let Ok(device) = devices.Item(device_index) else {
                continue;
            };
            let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else {
                continue;
            };
            let Ok(sessions) = manager.GetSessionEnumerator() else {
                continue;
            };
            for index in 0..sessions.GetCount()? {
                let session = sessions.GetSession(index)?;
                let control: IAudioSessionControl2 = session.cast()?;
                let Ok(pid) = control.GetProcessId() else {
                    continue;
                };
                // Never rename sessions belonging to Edge, Teams, or another
                // application just because they also use msedgewebview2.exe.
                if !ids.contains(&pid) {
                    continue;
                }
                let mut name = take_string(control.GetDisplayName()?);
                let mut icon = take_string(control.GetIconPath()?);
                if apply && name != NAME {
                    control.SetDisplayName(w!("YTM Listen"), std::ptr::null())?;
                    name = take_string(control.GetDisplayName()?);
                }
                if apply && icon != expected_path {
                    control.SetIconPath(PCWSTR(path_wide.as_ptr()), std::ptr::null())?;
                    icon = take_string(control.GetIconPath()?);
                }
                found.push(SessionIdentity {
                    name,
                    icon_matches: icon == expected_path,
                });
            }
        }
        Ok(found)
    }
}
