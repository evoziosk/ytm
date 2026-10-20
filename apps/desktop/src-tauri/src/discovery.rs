//! LAN DNS-SD advertisements carry public session metadata only. Admission still
//! happens over the normal native control connection after endpoint verification.
use mdns_sd::{ResolvedService, ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    net::Ipv4Addr,
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager};
use ytm_protocol::{ControlDiscovery, ControlMode, CONTROL_VERSION};

const SERVICE: &str = "_ytmlisten._tcp.local.";
static SCAN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static RESULTS: std::sync::Mutex<Vec<NearbyDevice>> = std::sync::Mutex::new(Vec::new());

#[derive(Clone, Debug, Serialize)]
pub struct NearbyDevice {
    pub id: String,
    pub name: String,
    pub address: String,
    pub mode: ControlMode,
}
pub struct Advertisement {
    daemon: ServiceDaemon,
    fullname: String,
    _multicast: MulticastLease,
}
impl Drop for Advertisement {
    fn drop(&mut self) {
        let _ = self.daemon.unregister(&self.fullname);
        let _ = self.daemon.shutdown();
    }
}
struct DaemonGuard(ServiceDaemon);
impl Drop for DaemonGuard {
    fn drop(&mut self) {
        let _ = self.0.shutdown();
    }
}
struct MulticastLease {
    #[cfg(target_os = "android")]
    app: AppHandle,
}
impl MulticastLease {
    fn new(_app: &AppHandle) -> Result<Self, String> {
        #[cfg(target_os = "android")]
        tauri_plugin_ytm_player::discovery_multicast(_app, true)?;
        Ok(Self {
            #[cfg(target_os = "android")]
            app: _app.clone(),
        })
    }
}
impl Drop for MulticastLease {
    fn drop(&mut self) {
        #[cfg(target_os = "android")]
        {
            let _ = tauri_plugin_ytm_player::discovery_multicast(&self.app, false);
        }
    }
}
fn local(ip: Ipv4Addr) -> bool {
    !ip.is_loopback() && (ip.is_private() || ip.is_link_local())
}
fn info(id: &str, name: &str, mode: ControlMode, port: u16) -> Result<ServiceInfo, String> {
    if !ytm_control::valid_secret(id) || name.len() > 64 || port == 0 {
        return Err("Invalid discovery metadata".into());
    }
    let properties = HashMap::from([
        ("version".to_owned(), CONTROL_VERSION.to_string()),
        ("session".to_owned(), id.to_owned()),
        ("name".to_owned(), name.to_owned()),
        (
            "mode".to_owned(),
            if mode == ControlMode::Personal {
                "personal"
            } else {
                "group"
            }
            .into(),
        ),
    ]);
    ServiceInfo::new(
        SERVICE,
        &format!("YTM-{}", &id[..16]),
        &format!("ytm-{}.local.", &id[..16]),
        "",
        port,
        properties,
    )
    .map(ServiceInfo::enable_addr_auto)
    .map_err(|e| e.to_string())
}
pub fn advertise(
    app: &AppHandle,
    id: &str,
    name: &str,
    mode: ControlMode,
    port: u16,
) -> Result<Advertisement, String> {
    let multicast = MulticastLease::new(app)?;
    let service = info(id, name, mode, port)?;
    let daemon = ServiceDaemon::new().map_err(|e| e.to_string())?;
    let fullname = service.get_fullname().to_owned();
    if let Err(error) = daemon.register(service) {
        let _ = daemon.shutdown();
        return Err(error.to_string());
    }
    Ok(Advertisement {
        daemon,
        fullname,
        _multicast: multicast,
    })
}
struct Candidate {
    id: String,
    mode: ControlMode,
    addresses: Vec<String>,
}
fn candidate(service: &ResolvedService, own: &str) -> Option<Candidate> {
    let id = service.get_property_val_str("session")?;
    if service.get_property_val_str("version")? != CONTROL_VERSION.to_string()
        || !ytm_control::valid_secret(id)
        || id == own
        || service.get_port() == 0
    {
        return None;
    }
    let mode = match service.get_property_val_str("mode")? {
        "personal" => ControlMode::Personal,
        "group" => ControlMode::Group,
        _ => return None,
    };
    let mut addresses: Vec<_> = service
        .get_addresses_v4()
        .into_iter()
        .filter(|ip| local(*ip))
        .collect();
    // Prefer an address on one of this controller's current subnets.
    let interfaces = if_addrs::get_if_addrs().unwrap_or_default();
    addresses.sort_by_key(|ip| {
        let same = interfaces.iter().any(|interface| match &interface.addr {
            if_addrs::IfAddr::V4(v4) => {
                u32::from(v4.ip) & u32::from(v4.netmask) == u32::from(*ip) & u32::from(v4.netmask)
            }
            _ => false,
        });
        (!same, *ip)
    });
    let addresses = addresses
        .into_iter()
        .take(4)
        .map(|ip| format!("http://{ip}:{}/", service.get_port()))
        .collect::<Vec<_>>();
    if addresses.is_empty() {
        return None;
    }
    Some(Candidate {
        id: id.into(),
        mode,
        addresses,
    })
}
async fn verify(candidate: Candidate) -> Option<NearbyDevice> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(900))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .ok()?;
    for address in candidate.addresses {
        let Ok(mut response) = client.get(format!("{address}session")).send().await else {
            continue;
        };
        if !response.status().is_success() {
            continue;
        }
        let mut bytes = Vec::new();
        let mut valid = true;
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) if bytes.len() + chunk.len() <= 8192 => {
                    bytes.extend_from_slice(&chunk)
                }
                Ok(None) => break,
                _ => {
                    valid = false;
                    break;
                }
            }
        }
        if !valid {
            continue;
        }
        let Ok(public) = serde_json::from_slice::<ControlDiscovery>(&bytes) else {
            continue;
        };
        if public.version == CONTROL_VERSION
            && public.session_id == candidate.id
            && public.mode == candidate.mode
            && !public.host_name.trim().is_empty()
            && public.host_name.len() <= 64
        {
            return Some(NearbyDevice {
                id: public.session_id,
                name: public.host_name,
                address,
                mode: public.mode,
            });
        }
    }
    None
}
async fn browse(own: &str, notify: impl Fn(&[NearbyDevice])) -> Result<Vec<NearbyDevice>, String> {
    let daemon = DaemonGuard(ServiceDaemon::new().map_err(|e| e.to_string())?);
    let events = daemon.0.browse(SERVICE).map_err(|e| e.to_string())?;
    let deadline = tokio::time::sleep(Duration::from_secs(8));
    tokio::pin!(deadline);
    let mut tasks = tokio::task::JoinSet::new();
    let mut seen = HashSet::new();
    let mut removed = HashSet::new();
    let mut names = HashMap::new();
    let mut devices = Vec::<NearbyDevice>::new();
    loop {
        tokio::select! {
            _ = &mut deadline => break,
            event = events.recv_async() => match event {
                Ok(ServiceEvent::ServiceResolved(service)) => {
                    if let Some(candidate) = candidate(&service, own) {
                        if seen.len() < 32 && tasks.len() < 8 && seen.insert(candidate.id.clone()) {
                            names.insert(service.get_fullname().to_owned(), candidate.id.clone());
                            removed.remove(&candidate.id);
                            tasks.spawn(verify(candidate));
                        }
                    }
                }
                Ok(ServiceEvent::ServiceRemoved(_, name)) => {
                    if let Some(id) = names.remove(&name) {
                        removed.insert(id.clone());
                        devices.retain(|device| device.id != id);
                        notify(&devices);
                    }
                }
                Err(_) => return Err("LAN discovery stopped. Retry or use the host address.".into()),
                _ => (),
            },
            result = tasks.join_next(), if !tasks.is_empty() => {
                if let Some(Ok(Some(device))) = result {
                    if devices.len() < 20 && !removed.contains(&device.id) {
                        devices.push(device);
                        devices.sort_by(|a,b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.id.cmp(&b.id)));
                        notify(&devices);
                    }
                }
            }
        }
    }
    tasks.abort_all();
    let _ = daemon.0.stop_browse(SERVICE);
    Ok(devices)
}
#[tauri::command]
pub async fn remote_discover(app: AppHandle) -> Result<Vec<NearbyDevice>, String> {
    let Ok(_guard) = SCAN.try_lock() else {
        return Ok(RESULTS.lock().expect("discovery lock").clone());
    };
    RESULTS.lock().expect("discovery lock").clear();
    let _multicast = MulticastLease::new(&app)?;
    let own = app
        .state::<crate::state::Core>()
        .control
        .lock()
        .expect("control lock")
        .session
        .as_ref()
        .map(|s| s.id.clone())
        .unwrap_or_default();
    browse(&own, |devices| {
        *RESULTS.lock().expect("discovery lock") = devices.to_vec();
        let _ = app.emit_to("settings", "nearby-devices", devices);
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    fn resolved(ips: &str) -> ResolvedService {
        ServiceInfo::new(
            SERVICE,
            "Test host",
            "test.local.",
            ips,
            9863,
            HashMap::from([
                ("session".into(), "a".repeat(64)),
                ("version".into(), "1".into()),
                ("mode".into(), "personal".into()),
            ]),
        )
        .unwrap()
        .as_resolved_service()
    }
    #[test]
    fn advertisements_are_public_and_candidates_are_local_compatible_and_not_self() {
        let metadata = info(&"a".repeat(64), "Living room", ControlMode::Personal, 9863).unwrap();
        let keys: HashSet<_> = metadata.get_properties().iter().map(|p| p.key()).collect();
        assert_eq!(keys, HashSet::from(["version", "session", "name", "mode"]));
        let service = resolved("192.168.7.2,8.8.8.8,127.0.0.1");
        assert_eq!(
            candidate(&service, "").unwrap().addresses,
            vec!["http://192.168.7.2:9863/"]
        );
        assert!(candidate(&service, &"a".repeat(64)).is_none());
        assert!(candidate(&resolved("8.8.8.8,127.0.0.1"), "").is_none());
        assert!(info("bad", "Host", ControlMode::Group, 9863).is_err());
        assert!(info(&"a".repeat(64), "Host", ControlMode::Group, 0).is_err());
    }
    #[tokio::test]
    async fn discovered_endpoint_must_match_the_advertised_session_and_never_redirect() {
        use axum::{routing::get, Router};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}/", listener.local_addr().unwrap());
        let server = Router::new().route(
            "/session",
            get(|| async {
                axum::Json(ControlDiscovery {
                    version: 1,
                    session_id: "a".repeat(64),
                    host_name: "Actual host name".into(),
                    mode: ControlMode::Personal,
                })
            }),
        );
        let task = tokio::spawn(async move { axum::serve(listener, server).await.unwrap() });
        let found = verify(Candidate {
            id: "a".repeat(64),
            mode: ControlMode::Personal,
            addresses: vec![address.clone()],
        })
        .await
        .unwrap();
        assert_eq!(found.name, "Actual host name");
        assert!(verify(Candidate {
            id: "b".repeat(64),
            mode: ControlMode::Personal,
            addresses: vec![address]
        })
        .await
        .is_none());
        task.abort();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}/", listener.local_addr().unwrap());
        let server = Router::new().route(
            "/session",
            get(|| async { axum::response::Redirect::temporary("https://example.com/") }),
        );
        let task = tokio::spawn(async move { axum::serve(listener, server).await.unwrap() });
        assert!(verify(Candidate {
            id: "a".repeat(64),
            mode: ControlMode::Personal,
            addresses: vec![address]
        })
        .await
        .is_none());
        task.abort();
    }
    #[tokio::test]
    #[ignore = "Explicit LAN multicast smoke test; requires a local IPv4 interface"]
    async fn multicast_round_trip() {
        let id = ytm_control::secret();
        let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let identity = id.clone();
        let server = axum::Router::new().route(
            "/session",
            axum::routing::get(move || {
                let id = identity.clone();
                async move {
                    axum::Json(ControlDiscovery {
                        version: 1,
                        session_id: id,
                        host_name: "Discovery smoke".into(),
                        mode: ControlMode::Group,
                    })
                }
            }),
        );
        let server_task = tokio::spawn(async move { axum::serve(listener, server).await.unwrap() });
        let daemon = DaemonGuard(ServiceDaemon::new().unwrap());
        let service = info(&id, "Discovery smoke", ControlMode::Group, port).unwrap();
        let fullname = service.get_fullname().to_owned();
        daemon.0.register(service).unwrap();
        let devices = browse("", |_| {}).await.unwrap();
        let _ = daemon.0.unregister(&fullname);
        server_task.abort();
        assert!(
            devices
                .iter()
                .any(|device| device.id == id && device.name == "Discovery smoke"),
            "Advertised host was not discovered and verified: {devices:?}"
        );
    }
}
