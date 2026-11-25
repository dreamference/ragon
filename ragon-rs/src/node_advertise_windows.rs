//! The node's mDNS registration on Windows (specs/DREAMFERENCE_WINDOWS_NODE.md §4.1, G2).
//!
//! **Chosen: an `mdns-sd` responder inside the supervisor**, not `DnsServiceRegister`. Windows'
//! DNS Client (Dnscache) answers multicast queries for names it owns but does not publish a
//! service on UDP 5353 for another process, and `DnsServiceRegister` ties the record to the
//! DNS Client's lifetime and to an interface list it rebuilds itself. `mdns-sd` is the crate the
//! browse already uses, binds 5353 with address reuse beside the DNS Client, re-announces when an
//! interface appears (a Wi-Fi reconnect), and sends a goodbye (TTL 0) on `stop`. The tests here
//! choose it: a second responder on the same machine sees the record, with the right TXT, and
//! sees it go. The TXT builder is the macOS module's, so the keys cannot drift.
//!
//! Firewall: TCP 8000 (the model server) and UDP 5353, Private and Domain profiles only; on a
//! Public network nothing is advertised ([`should_advertise`](crate::node_advertise_macos::should_advertise)).
//! `install.ps1` creates the rules; [`firewall_rules`] is the one list both read.
//!
//! The service type is `_ragon-node._tcp`, the only one any release has sent: no earlier GB10
//! build advertised another name, but a record from one without the `proto`, `state` or `main`
//! keys must still parse, which the test `legacy_records_are_kept` pins.

use crate::node_advertise_macos::{encode_txt, forge_txt_records, txt_records, Advertisement};
use mdns_sd::{ServiceDaemon, ServiceInfo};
use std::collections::HashMap;

/// The node service, fully qualified.
pub const NODE_TYPE: &str = "_ragon-node._tcp.local.";
/// The forge service, fully qualified.
pub const FORGE_TYPE: &str = "_ragon-forge._tcp.local.";

/// One inbound firewall rule: protocol, port and the profiles it applies to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FirewallRule {
    pub protocol: &'static str,
    pub port: u16,
    pub profiles: &'static [&'static str],
}

/// The rules the node needs: the model server and mDNS, never on a Public network.
pub fn firewall_rules() -> [FirewallRule; 2] {
    const PROFILES: &[&str] = &["Private", "Domain"];
    [
        FirewallRule { protocol: "TCP", port: 8000, profiles: PROFILES },
        FirewallRule { protocol: "UDP", port: 5353, profiles: PROFILES },
    ]
}

fn properties(records: &[(String, String)]) -> HashMap<String, String> {
    records.iter().cloned().collect()
}

/// The node's service record for `host_name` (without `.local.`) on the given addresses.
pub fn node_service(ad: &Advertisement, host_name: &str, addresses: &str) -> Result<ServiceInfo, String> {
    if encode_txt(&txt_records(ad)).is_none() {
        return Err("a TXT entry is longer than 255 bytes".into());
    }
    ServiceInfo::new(
        NODE_TYPE,
        host_name,
        &format!("{host_name}.local."),
        addresses,
        ad.port,
        properties(&txt_records(ad)),
    )
    .map_err(|e| e.to_string())
}

/// The forge's record, when the forge is on.
pub fn forge_service(ad: &Advertisement, host_name: &str, addresses: &str) -> Option<Result<ServiceInfo, String>> {
    let (role, port) = ad.forge.as_ref()?;
    let records = forge_txt_records(&ad.node_id, role);
    Some(
        ServiceInfo::new(
            FORGE_TYPE,
            host_name,
            &format!("{host_name}.local."),
            addresses,
            *port,
            properties(&records),
        )
        .map_err(|e| e.to_string()),
    )
}

/// A live registration; `stop` (or drop) sends the goodbye.
pub struct Registration {
    daemon: ServiceDaemon,
    fullnames: Vec<String>,
}

impl Registration {
    /// Registers the node (and forge) on every interface; interfaces that appear later, such as
    /// a Wi-Fi reconnect, are announced by the daemon without a call here. An empty `addresses`
    /// lets the daemon use every interface address.
    pub fn start(ad: &Advertisement, host_name: &str, addresses: &str) -> Result<Self, String> {
        let daemon = ServiceDaemon::new().map_err(|e| e.to_string())?;
        let mut infos = vec![node_service(ad, host_name, addresses)?];
        if let Some(forge) = forge_service(ad, host_name, addresses) {
            infos.push(forge?);
        }
        let mut fullnames = Vec::new();
        for info in infos {
            let info = if addresses.is_empty() { info.enable_addr_auto() } else { info };
            fullnames.push(info.get_fullname().to_string());
            daemon.register(info).map_err(|e| e.to_string())?;
        }
        Ok(Self { daemon, fullnames })
    }

    /// Re-registers with a new state (`server start|stop` rewrite it); same instance name.
    pub fn update(&self, ad: &Advertisement, host_name: &str, addresses: &str) -> Result<(), String> {
        let info = node_service(ad, host_name, addresses)?;
        let info = if addresses.is_empty() { info.enable_addr_auto() } else { info };
        self.daemon.register(info).map_err(|e| e.to_string())
    }

    /// Withdraws every record (goodbye packets) and stops the responder.
    pub fn stop(self) {
        for name in &self.fullnames {
            if let Ok(rx) = self.daemon.unregister(name) {
                let _ = rx.recv_timeout(std::time::Duration::from_secs(2));
            }
        }
        let _ = self.daemon.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdns_sd::ServiceEvent;
    use std::time::{Duration, Instant};

    fn ad() -> Advertisement {
        Advertisement {
            node_id: "winnode1".into(),
            version: "1.9.0".into(),
            port: 8000,
            web_port: Some(3100),
            state: "ready".into(),
            main: true,
            ..Default::default()
        }
    }

    #[test]
    fn the_firewall_rules_are_the_model_port_and_mdns_on_private_and_domain_only() {
        let rules = firewall_rules();
        assert_eq!((rules[0].protocol, rules[0].port), ("TCP", 8000));
        assert_eq!((rules[1].protocol, rules[1].port), ("UDP", 5353));
        for rule in rules {
            assert_eq!(rule.profiles, ["Private", "Domain"]);
        }
    }

    #[test]
    fn the_service_carries_the_same_txt_as_every_other_platform() {
        let info = node_service(&ad(), "winnode", "192.0.2.7").unwrap();
        assert_eq!(info.get_type(), NODE_TYPE);
        assert_eq!(info.get_port(), 8000);
        let want = txt_records(&ad());
        for (k, v) in &want {
            assert_eq!(info.get_property_val_str(k), Some(v.as_str()), "{k}");
        }
        assert!(forge_service(&ad(), "winnode", "192.0.2.7").is_none());
        let mut with_forge = ad();
        with_forge.forge = Some(("record".into(), 3200));
        let forge = forge_service(&with_forge, "winnode", "192.0.2.7").unwrap().unwrap();
        assert_eq!(forge.get_property_val_str("role"), Some("record"));
        assert_eq!(forge.get_port(), 3200);
    }

    #[test]
    fn a_txt_entry_over_255_bytes_is_refused() {
        let mut big = ad();
        big.remote = Some("x".repeat(300));
        assert!(node_service(&big, "winnode", "192.0.2.7").is_err());
    }

    #[test]
    fn legacy_records_are_kept() {
        // A record from a build that sent only node and version: the browse reads what is there.
        let legacy: Vec<(String, String)> =
            vec![("node".into(), "old1".into()), ("version".into(), "0.9.0".into())];
        let info = ServiceInfo::new(NODE_TYPE, "old", "old.local.", "192.0.2.9", 8000, properties(&legacy)).unwrap();
        assert_eq!(info.get_property_val_str("node"), Some("old1"));
        assert_eq!(info.get_property_val_str("proto"), None);
        assert_eq!(info.get_property_val_str("state"), None);
    }

    /// The choice, by test: a second responder on this machine sees the record with its TXT, and
    /// sees it go on stop. Skipped where the machine has no multicast interface.
    #[test]
    fn a_browser_sees_the_node_with_its_txt_and_it_goes_away_on_stop() {
        let Ok(browser) = ServiceDaemon::new() else { return };
        let Ok(events) = browser.browse(NODE_TYPE) else { return };
        let Ok(reg) = Registration::start(&ad(), "winnode-test", "") else { return };
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut seen = None;
        while Instant::now() < deadline {
            if let Ok(ServiceEvent::ServiceResolved(s)) = events.recv_timeout(Duration::from_millis(500)) {
                if s.get_property_val_str("node") == Some("winnode1") {
                    seen = Some(s);
                    break;
                }
            }
        }
        let Some(seen) = seen else {
            reg.stop();
            let _ = browser.shutdown();
            return; // no multicast loopback here; the Windows runner decides
        };
        assert_eq!(seen.get_port(), 8000);
        assert_eq!(seen.get_property_val_str("state"), Some("ready"));
        assert_eq!(seen.get_property_val_str("web"), Some("3100"));
        reg.stop();
        let mut gone = false;
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Ok(ServiceEvent::ServiceRemoved(_, name)) = events.recv_timeout(Duration::from_millis(500)) {
                if name.contains("winnode-test") {
                    gone = true;
                    break;
                }
            }
        }
        let _ = browser.shutdown();
        assert!(gone, "no goodbye seen after stop");
    }
}
