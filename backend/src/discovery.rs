use std::{collections::HashMap, net::IpAddr, net::Ipv4Addr, thread};

use chrono::Utc;
use mdns_sd::{ResolvedService, ServiceDaemon, ServiceEvent, ServiceInfo};

use crate::{error::AppError, models::PeerPresence, state::SharedState};

const SERVICE_TYPE: &str = "_csm-codex._tcp.local.";

pub struct LanDiscoveryHandle {
    daemon: ServiceDaemon,
    service_fullname: Option<String>,
}

impl Drop for LanDiscoveryHandle {
    fn drop(&mut self) {
        if let Some(fullname) = self.service_fullname.as_deref() {
            let _ = self.daemon.unregister(fullname);
        }
        let _ = self.daemon.shutdown();
    }
}

pub fn start(
    state: SharedState,
    local_peer_id: String,
    local_display_name: String,
) -> Result<Option<LanDiscoveryHandle>, AppError> {
    if !state.config.lan_discovery_enabled {
        return Ok(None);
    }

    let daemon = ServiceDaemon::new()
        .map_err(|error| AppError::External(format!("failed to start mDNS daemon: {error}")))?;
    let receiver = daemon
        .browse(SERVICE_TYPE)
        .map_err(|error| AppError::External(format!("failed to browse LAN peers: {error}")))?;
    let service_fullname =
        register_local_presence(&daemon, &state, &local_peer_id, local_display_name)?;
    thread::Builder::new()
        .name("csm-mdns-discovery".to_string())
        .spawn(move || {
            while let Ok(event) = receiver.recv() {
                match event {
                    ServiceEvent::ServiceResolved(service) => {
                        let Some(presence) = presence_from_service(&service) else {
                            continue;
                        };
                        if presence.peer_id == local_peer_id {
                            continue;
                        }
                        let mut inner = state.inner.blocking_write();
                        inner
                            .peer_presence
                            .insert(presence.peer_id.clone(), presence);
                    }
                    ServiceEvent::ServiceRemoved(_, fullname) => {
                        let mut inner = state.inner.blocking_write();
                        inner
                            .peer_presence
                            .retain(|_, presence| presence.service_name != fullname);
                    }
                    _ => {}
                }
            }
        })
        .map_err(|error| {
            AppError::External(format!("failed to start LAN discovery thread: {error}"))
        })?;

    Ok(Some(LanDiscoveryHandle {
        daemon,
        service_fullname: Some(service_fullname),
    }))
}

fn register_local_presence(
    daemon: &ServiceDaemon,
    state: &SharedState,
    peer_id: &str,
    display_name: String,
) -> Result<String, AppError> {
    let instance_name = sanitize_instance_name(&display_name);
    let host_name = format!("{peer_id}.local.");
    let properties = HashMap::from([
        ("peerId".to_string(), peer_id.to_string()),
        ("displayName".to_string(), display_name),
        ("version".to_string(), env!("CARGO_PKG_VERSION").to_string()),
    ]);
    let service = ServiceInfo::new(
        SERVICE_TYPE,
        &instance_name,
        &host_name,
        IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        state.config.bind_addr.port(),
        properties,
    )
    .map_err(|error| AppError::External(format!("failed to build peer presence: {error}")))?
    .enable_addr_auto();
    let fullname = service.get_fullname().to_string();

    daemon.register(service).map_err(|error| {
        AppError::External(format!("failed to announce peer presence: {error}"))
    })?;

    Ok(fullname)
}

fn presence_from_service(service: &ResolvedService) -> Option<PeerPresence> {
    let peer_id = service.get_property_val_str("peerId")?.to_string();
    let display_name = service
        .get_property_val_str("displayName")
        .map(str::to_string)
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| service.get_fullname().to_string());
    let version = service.get_property_val_str("version").map(str::to_string);
    let ip = service
        .get_addresses_v4()
        .into_iter()
        .find(|ip| !ip.is_loopback())
        .or_else(|| service.get_addresses_v4().into_iter().next())?;
    let port = service.get_port();

    Some(PeerPresence {
        peer_id,
        service_name: service.get_fullname().to_string(),
        display_name,
        version,
        base_url: format!("http://{ip}:{port}"),
        host_name: service.get_hostname().to_string(),
        port,
        last_seen_at: Utc::now(),
    })
}

fn sanitize_instance_name(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    let sanitized = sanitized.trim_matches('-');

    if sanitized.is_empty() {
        "traceway".to_string()
    } else {
        sanitized.chars().take(40).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::sanitize_instance_name;

    #[test]
    fn discovery_instance_name_is_ascii_and_bounded() {
        assert_eq!(sanitize_instance_name("Alice MacBook"), "Alice-MacBook");
        assert_eq!(sanitize_instance_name("  "), "traceway");
        assert!(sanitize_instance_name(&"a".repeat(80)).len() <= 40);
    }
}
