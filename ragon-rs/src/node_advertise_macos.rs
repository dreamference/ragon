//! The node's Bonjour registration on macOS (specs/DREAMFERENCE_RAGON_NODE.md §5).
//!
//! The node registers `_ragon-node._tcp` (and the forge's `_ragon-forge._tcp` when the forge is on)
//! with the system's mDNSResponder through `DNSServiceRegister` in libSystem, so no second responder
//! listens on UDP 5353; `mdns-sd` stays for the browse. The TXT keys are those of the Python
//! `NodeServiceFile`; both are tested against `vectors/node-txt-keys.json`. Nothing is registered
//! on a network marked public. The TXT builder is plain Rust and runs on every platform; only the
//! registration is macOS.

/// The contract version advertised; the same as `NodeServiceFile`'s `PROTO`.
pub const PROTO: u32 = 1;

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

/// The DNS-SD TXT wire format: each entry `key=value` behind a length byte. None when an entry
/// is longer than 255 bytes.
pub fn encode_txt(records: &[(String, String)]) -> Option<Vec<u8>> {
    let mut wire = Vec::new();
    for (key, value) in records {
        let entry = format!("{key}={value}");
        let len = u8::try_from(entry.len()).ok()?;
        wire.push(len);
        wire.extend_from_slice(entry.as_bytes());
    }
    Some(wire)
}

/// Whether to advertise at all: never on a network marked public.
pub fn should_advertise(network_is_public: bool) -> bool {
    !network_is_public
}

#[cfg(target_os = "macos")]
pub use registration::{network_is_public, Registration};

#[cfg(target_os = "macos")]
mod registration {
    use super::{encode_txt, forge_txt_records, should_advertise, txt_records, Advertisement};
    use std::ffi::{c_char, c_void, CString};
    use std::ptr;

    type DnsServiceRef = *mut c_void;

    #[link(name = "System")]
    unsafe extern "C" {
        fn DNSServiceRegister(
            sd_ref: *mut DnsServiceRef,
            flags: u32,
            interface_index: u32,
            name: *const c_char,
            reg_type: *const c_char,
            domain: *const c_char,
            host: *const c_char,
            port: u16,
            txt_len: u16,
            txt_record: *const c_void,
            callback: *const c_void,
            context: *mut c_void,
        ) -> i32;
        fn DNSServiceRefDeallocate(sd_ref: DnsServiceRef);
    }

    /// Whether the owner marked the network public: `DREAMFERENCE_NETWORK_PUBLIC=1`, which the
    /// installer's network check sets (the system has no call that says it without a framework).
    pub fn network_is_public() -> bool {
        std::env::var("DREAMFERENCE_NETWORK_PUBLIC").is_ok_and(|v| v == "1")
    }

    /// The live registrations; dropping it (or `stop`) withdraws them from mDNSResponder.
    pub struct Registration {
        refs: Vec<DnsServiceRef>,
    }

    fn register(reg_type: &str, port: u16, txt: &[(String, String)]) -> Result<DnsServiceRef, String> {
        let wire = encode_txt(txt).ok_or("a TXT entry is longer than 255 bytes")?;
        let reg_type = CString::new(reg_type).map_err(|e| e.to_string())?;
        let mut sd: DnsServiceRef = ptr::null_mut();
        // Empty name: the computer's name. Interface 0: every interface. Port in network order.
        let err = unsafe {
            DNSServiceRegister(
                &mut sd,
                0,
                0,
                ptr::null(),
                reg_type.as_ptr(),
                ptr::null(),
                ptr::null(),
                port.to_be(),
                wire.len() as u16,
                wire.as_ptr().cast(),
                ptr::null(),
                ptr::null_mut(),
            )
        };
        if err != 0 {
            return Err(format!("DNSServiceRegister failed with {err}"));
        }
        Ok(sd)
    }

    impl Registration {
        /// Registers the node (and the forge). `Ok(None)` on a public network.
        pub fn start(ad: &Advertisement) -> Result<Option<Registration>, String> {
            if !should_advertise(network_is_public()) {
                return Ok(None);
            }
            let mut reg = Registration { refs: Vec::new() };
            reg.refs.push(register("_ragon-node._tcp", ad.port, &txt_records(ad))?);
            if let Some((role, port)) = &ad.forge {
                reg.refs.push(register("_ragon-forge._tcp", *port, &forge_txt_records(&ad.node_id, role))?);
            }
            Ok(Some(reg))
        }

        /// Withdraws the registrations.
        pub fn stop(self) {}
    }

    impl Drop for Registration {
        fn drop(&mut self) {
            for sd in self.refs.drain(..) {
                unsafe { DNSServiceRefDeallocate(sd) };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vector() -> serde_json::Value {
        serde_json::from_str(include_str!("../vectors/node-txt-keys.json")).unwrap()
    }

    #[test]
    fn the_txt_keys_are_the_vectors() {
        let v = vector();
        let ad = Advertisement {
            node_id: v["node_id"].as_str().unwrap().into(),
            version: v["version"].as_str().unwrap().into(),
            port: 8000,
            web_port: Some(v["web_port"].as_u64().unwrap() as u16),
            search_port: Some(v["search_port"].as_u64().unwrap() as u16),
            state: v["state"].as_str().unwrap().into(),
            main: true,
            remote: Some(v["remote"].as_str().unwrap().into()),
            forge: None,
        };
        let records = txt_records(&ad);
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
    fn the_wire_format_has_length_bytes_and_nothing_is_advertised_on_a_public_network() {
        assert_eq!(encode_txt(&[("a".into(), "b".into())]), Some(vec![3, b'a', b'=', b'b']));
        assert_eq!(encode_txt(&[("k".into(), "x".repeat(300))]), None);
        assert!(!should_advertise(true) && should_advertise(false));
    }
}
