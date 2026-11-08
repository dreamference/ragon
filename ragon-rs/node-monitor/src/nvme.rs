//! NVMe health (§3.6) as udisks2 last read it, over the system bus without privilege. The monitor
//! never asks the drive for a fresh reading; it reports the cached one and its age. "Media errors
//! rose" needs history and waits for the store.

use crate::host::Host;
use crate::{Item, State, Weight};
use serde_json::{Value, json};
use std::time::Duration;

const UDISKS: &str = "org.freedesktop.UDisks2";
const CONTROLLER: &str = "org.freedesktop.UDisks2.NVMe.Controller";
const STALE_SECS: u64 = 7 * 24 * 3600;

/// The value inside a `busctl --json` variant or array wrapper.
fn plain(v: &Value) -> &Value {
    let mut v = v;
    while let Some(d) = v.get("data") {
        v = d;
    }
    while let Some([only]) = v.as_array().map(Vec::as_slice) {
        v = only;
    }
    v
}

/// The object paths of the NVMe controllers in `GetManagedObjects`' JSON, with their `SmartUpdated`.
pub fn controllers(json: &str) -> Option<Vec<(String, u64)>> {
    let doc: Value = serde_json::from_str(json).ok()?;
    let objects = plain(&doc).as_object()?;
    Some(
        objects
            .iter()
            .filter_map(|(path, ifaces)| {
                let props = ifaces.get(CONTROLLER)?;
                let updated = plain(props.get("SmartUpdated")?).as_u64().unwrap_or(0);
                Some((path.clone(), updated))
            })
            .collect(),
    )
}

fn num(attrs: &Value, key: &str) -> Option<f64> {
    plain(attrs.get(key)?).as_f64()
}

/// The state of one controller from its `SmartGetAttributes` JSON and the age of the reading.
pub fn judge(attrs_json: &str, updated: u64, now: u64) -> Option<(State, String, Value)> {
    let doc: Value = serde_json::from_str(attrs_json).ok()?;
    let attrs = plain(&doc);
    attrs.as_object()?;
    let spare = num(attrs, "avail_spare");
    let spare_min = num(attrs, "spare_thresh");
    let used = num(attrs, "percent_used");
    let critical = num(attrs, "crit_warning").unwrap_or(0.0);
    let temp = num(attrs, "temp_sensor1").or_else(|| num(attrs, "temperature"));
    let warn = num(attrs, "wctemp");
    let age = now.saturating_sub(updated);
    let values = json!({
        "critical_warning": critical, "available_spare": spare, "spare_threshold": spare_min,
        "percent_used": used, "media_errors": num(attrs, "media_errors"),
        "temperature_k": temp, "age_secs": age,
    });
    let (state, why) = if critical != 0.0 {
        (State::Down, "critical warning set".to_string())
    } else if spare.zip(spare_min).is_some_and(|(s, t)| s <= t) {
        (
            State::Down,
            "available spare at or below its threshold".to_string(),
        )
    } else if used.is_some_and(|u| u >= 90.0) {
        (
            State::Degraded,
            "90 % or more of the drive's life used".to_string(),
        )
    } else if temp.zip(warn).is_some_and(|(t, w)| w > 0.0 && t >= w) {
        (
            State::Degraded,
            "temperature at or above the drive's warning level".to_string(),
        )
    } else if age > STALE_SECS {
        (
            State::Degraded,
            format!("the cached reading is {} days old", age / 86400),
        )
    } else {
        (State::Ok, "healthy".to_string())
    };
    Some((state, why, values))
}

pub fn probe(host: &dyn Host) -> Vec<Item> {
    let name = "nvme";
    let unknown = |why: String| vec![Item::new(name, State::Unknown, Weight::Item, why)];
    let t = Duration::from_secs(5);
    let managed = host.run(
        "busctl",
        &[
            "--system",
            "--json=short",
            "call",
            UDISKS,
            "/org/freedesktop/UDisks2",
            "org.freedesktop.DBus.ObjectManager",
            "GetManagedObjects",
        ],
        t,
    );
    let list = match managed {
        Ok(o) if o.success => o.stdout,
        Ok(_) => return unknown("udisks2 did not answer".into()),
        Err(e) => return unknown(format!("busctl: {}", e.kind())),
    };
    let Some(found) = controllers(&list) else {
        return unknown("udisks2 answer not understood".into());
    };
    if found.is_empty() {
        return unknown("udisks2 knows no NVMe drive".into());
    }
    let now = host.now_unix();
    found
        .into_iter()
        .map(|(path, updated)| {
            let id = format!("nvme:{}", path.rsplit('/').next().unwrap_or(&path));
            let call = host.run(
                "busctl",
                &[
                    "--system",
                    "--json=short",
                    "call",
                    UDISKS,
                    &path,
                    CONTROLLER,
                    "SmartGetAttributes",
                    "a{sv}",
                ],
                t,
            );
            match call
                .ok()
                .filter(|o| o.success)
                .and_then(|o| judge(&o.stdout, updated, now))
            {
                Some((state, why, values)) => {
                    Item::new(&id, state, Weight::Item, why).with_values(values)
                }
                None => Item::new(&id, State::Unknown, Weight::Item, "no SMART reading"),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeHost;

    const PATH: &str = "/org/freedesktop/UDisks2/drives/nvme0";
    const LIST: &str = r#"{"type":"a{oa{sa{sv}}}","data":[{"/org/freedesktop/UDisks2/drives/nvme0":{"org.freedesktop.UDisks2.NVMe.Controller":{"SmartUpdated":{"type":"t","data":1000}}},"/org/freedesktop/UDisks2/drives/x":{"org.freedesktop.UDisks2.Drive":{}}}]}"#;

    fn attrs(overrides: &str) -> String {
        let base = r#""avail_spare":{"type":"y","data":100},"spare_thresh":{"type":"y","data":10},"percent_used":{"type":"y","data":5},"crit_warning":{"type":"y","data":0},"temp_sensor1":{"type":"q","data":310},"wctemp":{"type":"q","data":343}"#;
        let json = format!(r#"{{"type":"a{{sv}}","data":[{{{base}}}]}}"#);
        if overrides.is_empty() {
            return json;
        }
        let v: Value = serde_json::from_str(&json).unwrap();
        let mut v = v;
        let o: Value = serde_json::from_str(overrides).unwrap();
        for (k, x) in o.as_object().unwrap() {
            v["data"][0][k] = json!({"type":"y","data":x});
        }
        v.to_string()
    }

    fn host(attrs_json: &str) -> FakeHost {
        FakeHost {
            now: 2000,
            ..Default::default()
        }
        .command(
            "busctl --system --json=short call org.freedesktop.UDisks2 /org/freedesktop/UDisks2 org.freedesktop.DBus.ObjectManager GetManagedObjects",
            LIST,
        )
        .command(
            &format!("busctl --system --json=short call org.freedesktop.UDisks2 {PATH} org.freedesktop.UDisks2.NVMe.Controller SmartGetAttributes a{{sv}}"),
            attrs_json,
        )
    }

    #[test]
    fn finds_the_controller_and_reads_it() {
        assert_eq!(controllers(LIST).unwrap(), vec![(PATH.to_string(), 1000)]);
        let items = probe(&host(&attrs("")));
        assert_eq!(
            (items[0].name.as_str(), items[0].state),
            ("nvme:nvme0", State::Ok)
        );
    }

    #[test]
    fn thresholds_follow_the_spec() {
        let state = |o: &str| probe(&host(&attrs(o)))[0].state;
        assert_eq!(state(r#"{"crit_warning":1}"#), State::Down);
        assert_eq!(state(r#"{"avail_spare":10}"#), State::Down);
        assert_eq!(state(r#"{"percent_used":90}"#), State::Degraded);
        assert_eq!(state(r#"{"temp_sensor1":343}"#), State::Degraded);
    }

    #[test]
    fn an_old_reading_is_degraded() {
        let (state, why, _) = judge(&attrs(""), 0, 8 * 86400).unwrap();
        assert_eq!(state, State::Degraded);
        assert!(why.contains("8 days"));
    }

    #[test]
    fn missing_sources_are_unknown() {
        assert_eq!(probe(&FakeHost::default())[0].state, State::Unknown);
        let none = FakeHost::default().command(
            "busctl --system --json=short call org.freedesktop.UDisks2 /org/freedesktop/UDisks2 org.freedesktop.DBus.ObjectManager GetManagedObjects",
            r#"{"type":"a{oa{sa{sv}}}","data":[{}]}"#,
        );
        assert_eq!(probe(&none)[0].state, State::Unknown);
        let no_attrs = host("not json");
        assert_eq!(probe(&no_attrs)[0].state, State::Unknown);
    }
}
