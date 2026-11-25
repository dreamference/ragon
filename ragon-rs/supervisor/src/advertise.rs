//! The node's LAN advertisement: `_ragon-node._tcp` (and the forge's `_ragon-forge._tcp`) from an
//! `mdns-sd` responder inside the supervisor (specs/DREAMFERENCE_WINDOWS_NODE.md §4.2, owner
//! ruling WN1: no Python on a Windows node). The record is withdrawn, with goodbye packets, when
//! the responder is dropped, so it is gone when the supervisor stops; `mdns-sd` re-reads the
//! host's addresses every few seconds and re-announces when they change, which is what a Wi-Fi
//! reconnect needs.
//!
//! The TXT keys and their order are those of the Python `NodeServiceFile.render` and of
//! `vectors/node-txt-keys.json` (the same vector the macOS registration is tested against); the
//! TXT builder and the supervisor-state mapping are plain Rust and run everywhere.

use std::net::IpAddr;
use std::time::Duration;

use mdns_sd::{ServiceDaemon, ServiceInfo};

use crate::core::State;

/// The contract version advertised; the same as `NodeServiceFile`'s `PROTO`.
pub const PROTO: u32 = 1;
pub const NODE_TYPE: &str = "_ragon-node._tcp.local.";
pub const FORGE_TYPE: &str = "_ragon-forge._tcp.local.";

/// What a node advertises.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Advertisement {
    pub node_id: String,
    pub version: String,
    /// The model server's port (the SRV port).
    pub port: u16,
    pub web_port: Option<u16>,
    pub search_port: Option<u16>,
    /// `stopped`, `loading` or `ready`.
    pub state: String,
    pub main: bool,
    pub remote: Option<String>,
    /// The forge's role (`record` or `mirror`) and HTTP port; None when the forge is off.
    pub forge: Option<(String, u16)>,
}

/// The node record's TXT entries in `NodeServiceFile.render`'s order.
pub fn txt_records(ad: &Advertisement) -> Vec<(String, String)> {
    let mut out = vec![
        ("proto".to_string(), PROTO.to_string()),
        ("node".to_string(), ad.node_id.clone()),
        ("version".to_string(), ad.version.clone()),
    ];
    if let Some(web) = ad.web_port {
        out.push(("web".to_string(), web.to_string()));
    }
    if let Some(search) = ad.search_port {
        out.push(("search".to_string(), search.to_string()));
    }
    out.push(("state".to_string(), ad.state.clone()));
    if ad.main {
        out.push(("main".to_string(), "1".to_string()));
    }
    if let Some(remote) = ad.remote.as_deref().filter(|r| !r.is_empty()) {
        out.push(("remote".to_string(), remote.to_string()));
    }
    out
}

/// The forge record's TXT entries.
pub fn forge_txt_records(node_id: &str, role: &str) -> Vec<(String, String)> {
    vec![("node".to_string(), node_id.to_string()), ("role".to_string(), role.to_string())]
}

/// The advertised `state` for the supervisor's: `ready` only while the engine answers its probe,
/// `loading` while it starts or is restarted, `stopped` otherwise (a refused or stopped engine).
pub fn advertised_state(state: State) -> &'static str {
    match state {
        State::Healthy | State::Unhealthy => "ready",
        State::Starting | State::Waiting => "loading",
        State::Stopped | State::Refused => "stopped",
    }
}

/// Whether to advertise at all: never on a network marked public.
pub fn should_advertise(network_is_public: bool) -> bool {
    !network_is_public
}

/// The owner (or the installer's network check) marked the network public:
/// `DREAMFERENCE_NETWORK_PUBLIC=1`, the same switch as the macOS registration.
pub fn network_is_public() -> bool {
    std::env::var("DREAMFERENCE_NETWORK_PUBLIC").is_ok_and(|v| v == "1")
}

/// A DNS label from a machine name: lowercase letters, digits and hyphens.
pub fn host_label(raw: &str) -> String {
    let label: String = raw
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let label = label.trim_matches('-').to_string();
    if label.is_empty() { "ragon-node".to_string() } else { label }
}

/// The machine's name from the environment (`COMPUTERNAME` on Windows, `HOSTNAME` elsewhere).
pub fn machine_name() -> String {
    let raw = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_default();
    host_label(&raw)
}

/// The service instance name: the machine name and the first four characters of the node id.
pub fn instance_name(machine: &str, node_id: &str) -> String {
    let tail: String = node_id.chars().filter(char::is_ascii_alphanumeric).take(4).collect::<String>().to_ascii_lowercase();
    if tail.is_empty() { machine.to_string() } else { format!("{machine}-{tail}") }
}

fn service_info(ty: &str, instance: &str, host: &str, port: u16, txt: &[(String, String)]) -> Result<ServiceInfo, String> {
    let props: Vec<(&str, &str)> = txt.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    // No fixed address: the responder fills in the host's own and follows them when they change.
    let info = ServiceInfo::new(ty, instance, &format!("{host}.local."), &[] as &[IpAddr], port, &props[..]).map_err(|e| e.to_string())?;
    Ok(info.enable_addr_auto())
}

/// The live registrations. Dropping it withdraws them (goodbye packets) and stops the responder.
pub struct Responder {
    daemon: ServiceDaemon,
    machine: String,
    registered: Vec<String>,
}

impl Responder {
    /// Starts the responder and registers the node (and the forge). `Ok(None)` on a public network.
    pub fn start(ad: &Advertisement) -> Result<Option<Responder>, String> {
        Responder::start_with(ad, ServiceDaemon::new().map_err(|e| e.to_string())?, should_advertise(network_is_public()))
    }

    /// As [`Responder::start`] with a given daemon (the tests use one on a private port).
    pub fn start_with(ad: &Advertisement, daemon: ServiceDaemon, allowed: bool) -> Result<Option<Responder>, String> {
        if !allowed {
            let _ = daemon.shutdown();
            return Ok(None);
        }
        let mut responder = Responder { daemon, machine: machine_name(), registered: Vec::new() };
        responder.register(ad)?;
        Ok(Some(responder))
    }

    fn register(&mut self, ad: &Advertisement) -> Result<(), String> {
        let instance = instance_name(&self.machine, &ad.node_id);
        let node = service_info(NODE_TYPE, &instance, &self.machine, ad.port, &txt_records(ad))?;
        self.registered.push(node.get_fullname().to_string());
        self.daemon.register(node).map_err(|e| e.to_string())?;
        if let Some((role, port)) = &ad.forge {
            let forge = service_info(FORGE_TYPE, &instance, &self.machine, *port, &forge_txt_records(&ad.node_id, role))?;
            self.registered.push(forge.get_fullname().to_string());
            self.daemon.register(forge).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Announces a changed record (the state moved, a port changed): the same names, new TXT.
    pub fn update(&mut self, ad: &Advertisement) -> Result<(), String> {
        self.registered.clear();
        self.register(ad)
    }

    /// Withdraws the registrations and waits (briefly) for the goodbye to go out.
    pub fn stop(mut self) {
        self.withdraw();
    }

    fn withdraw(&mut self) {
        for name in self.registered.drain(..) {
            if let Ok(done) = self.daemon.unregister(&name) {
                let _ = done.recv_timeout(Duration::from_secs(2));
            }
        }
        if let Ok(done) = self.daemon.shutdown() {
            let _ = done.recv_timeout(Duration::from_secs(2));
        }
    }
}

impl Drop for Responder {
    fn drop(&mut self) {
        self.withdraw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vector() -> serde_json::Value {
        serde_json::from_str(include_str!("../../vectors/node-txt-keys.json")).unwrap()
    }

    fn vector_ad(v: &serde_json::Value) -> Advertisement {
        Advertisement {
            node_id: v["node_id"].as_str().unwrap().into(),
            version: v["version"].as_str().unwrap().into(),
            port: 8000,
            web_port: Some(v["web_port"].as_u64().unwrap() as u16),
            search_port: Some(v["search_port"].as_u64().unwrap() as u16),
            state: v["state"].as_str().unwrap().into(),
            main: true,
            remote: Some(v["remote"].as_str().unwrap().into()),
            forge: None,
        }
    }

    #[test]
    fn the_txt_keys_are_the_vectors() {
        let v = vector();
        let records = txt_records(&vector_ad(&v));
        let keys: Vec<&str> = records.iter().map(|(k, _)| k.as_str()).collect();
        let want: Vec<&str> = v["keys"].as_array().unwrap().iter().map(|k| k.as_str().unwrap()).collect();
        assert_eq!(keys, want);
        for (k, val) in &records {
            assert_eq!(val, v["values"][k].as_str().unwrap());
        }
        let forge: Vec<String> = forge_txt_records("x", "record").into_iter().map(|(k, _)| k).collect();
        let want: Vec<&str> = v["forge"]["keys"].as_array().unwrap().iter().map(|k| k.as_str().unwrap()).collect();
        assert_eq!(forge, want);
    }

    #[test]
    fn unshared_records_are_left_out() {
        let ad = Advertisement { node_id: "n".into(), version: "1".into(), state: "stopped".into(), ..Default::default() };
        let keys: Vec<String> = txt_records(&ad).into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys, ["proto", "node", "version", "state"]);
    }

    #[test]
    fn the_supervisor_state_maps_to_the_advertised_one() {
        assert_eq!(advertised_state(State::Healthy), "ready");
        assert_eq!(advertised_state(State::Starting), "loading");
        assert_eq!(advertised_state(State::Waiting), "loading");
        assert_eq!(advertised_state(State::Refused), "stopped");
        assert_eq!(advertised_state(State::Stopped), "stopped");
        assert!(!should_advertise(true) && should_advertise(false));
    }

    #[test]
    fn names_are_dns_labels() {
        assert_eq!(host_label("STRIX_Halo 01"), "strix-halo-01");
        assert_eq!(host_label("  "), "ragon-node");
        assert_eq!(instance_name("box", "AB-CD12-9"), "box-abcd");
        assert_eq!(instance_name("box", ""), "box");
    }

    /// A browse on the same host sees what the responder announces, then sees it go. Needs a
    /// multicast-capable interface, so it is ignored by default: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn a_browser_sees_the_record_and_its_withdrawal() {
        use mdns_sd::ServiceEvent;
        let v = vector();
        let ad = vector_ad(&v);
        let browser = ServiceDaemon::new().unwrap();
        let rx = browser.browse(NODE_TYPE).unwrap();
        let responder = Responder::start_with(&ad, ServiceDaemon::new().unwrap(), true).unwrap().unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        let mut seen = None;
        while seen.is_none() && std::time::Instant::now() < deadline {
            if let Ok(ServiceEvent::ServiceResolved(info)) = rx.recv_timeout(Duration::from_secs(1))
                && info.get_property_val_str("node") == Some(ad.node_id.as_str())
            {
                seen = Some(info);
            }
        }
        let info = seen.expect("the record was not seen");
        assert_eq!(info.get_port(), 8000);
        for (k, val) in txt_records(&ad) {
            assert_eq!(info.get_property_val_str(&k), Some(val.as_str()), "{k}");
        }
        responder.stop();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut gone = false;
        while !gone && std::time::Instant::now() < deadline {
            if let Ok(ServiceEvent::ServiceRemoved(_, _)) = rx.recv_timeout(Duration::from_secs(1)) {
                gone = true;
            }
        }
        assert!(gone, "no goodbye was seen");
        let _ = browser.shutdown();
    }
}
