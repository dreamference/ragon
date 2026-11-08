//! Units (§3.8): `systemctl --user show` for the product's units that are enabled. One table; a
//! feature that adds a unit adds a row. A unit that is not enabled (or not installed) is `off`.
//! Not in Phase 1: `ragon-signal@` (its own account), sidecars, Docker, D-Bus events.

use crate::host::Host;
use crate::{Item, State, Weight};
use serde_json::json;
use std::collections::HashMap;
use std::time::Duration;

/// The product's user units that Phase 1 watches.
pub const UNITS: &[&str] = &[
    "ragon-web.service",
    "ragon-chat.service",
    "ragon-forge.service",
    "ragon-remote-control.service",
    "ragon-remote-certificate.timer",
];

const PROPERTIES: &str = "LoadState,ActiveState,SubState,Result,NRestarts,UnitFileState";
const TIMEOUT: Duration = Duration::from_secs(5);

pub fn probe(host: &dyn Host) -> Vec<Item> {
    UNITS.iter().map(|unit| unit_item(host, unit)).collect()
}

/// `Key=Value` lines.
pub fn parse_show(text: &str) -> HashMap<&str, &str> {
    text.lines().filter_map(|l| l.split_once('=')).collect()
}

fn unit_item(host: &dyn Host, unit: &str) -> Item {
    let name = format!("unit:{unit}");
    let out = match host.run(
        "systemctl",
        &["--user", "show", "-p", PROPERTIES, unit],
        TIMEOUT,
    ) {
        Ok(o) if o.success => o,
        Ok(_) => return Item::new(&name, State::Unknown, Weight::Item, "systemctl show failed"),
        Err(e) => {
            return Item::new(
                &name,
                State::Unknown,
                Weight::Item,
                format!("systemctl: {}", e.kind()),
            );
        }
    };
    let p = parse_show(&out.stdout);
    let get = |k: &str| p.get(k).copied().unwrap_or("");
    let (load, active, sub, result) = (
        get("LoadState"),
        get("ActiveState"),
        get("SubState"),
        get("Result"),
    );
    let enabled = matches!(
        get("UnitFileState"),
        "enabled" | "enabled-runtime" | "linked" | "linked-runtime"
    );
    let restarts: u64 = get("NRestarts").parse().unwrap_or(0);
    if load != "loaded" || !enabled {
        return Item::new(&name, State::Off, Weight::Item, "not enabled");
    }
    let is_timer = unit.ends_with(".timer");
    // "restarted in the last hour" and "a timer's last run" need history; the count since the unit
    // started and the unit's own Result stand in for them.
    let (state, detail) = match active {
        "failed" => (State::Down, format!("failed ({result})")),
        "inactive" => (State::Down, "inactive while enabled".to_string()),
        "active" if restarts > 0 => (
            State::Degraded,
            format!("{sub}, restarted {restarts} times"),
        ),
        "active" if is_timer && !matches!(result, "" | "success") => {
            (State::Degraded, format!("{sub}, last result {result}"))
        }
        "active" => (State::Ok, sub.to_string()),
        other => (State::Degraded, format!("{other} ({sub})")),
    };
    Item::new(&name, state, Weight::Item, detail)
        .with_values(json!({ "active": active, "sub": sub, "restarts": restarts }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeHost;

    fn show(unit: &str, body: &str) -> (String, String) {
        (
            format!("systemctl --user show -p {PROPERTIES} {unit}"),
            body.to_string(),
        )
    }

    fn host(entries: &[(&str, &str)]) -> FakeHost {
        let mut h = FakeHost::default();
        for (unit, body) in entries {
            let (line, out) = show(unit, body);
            h = h.command(&line, &out);
        }
        h
    }

    const RUNNING: &str = "LoadState=loaded\nActiveState=active\nSubState=running\nResult=success\nNRestarts=0\nUnitFileState=enabled\n";

    fn state_of(body: &str) -> (State, String) {
        let items = probe(&host(&[("ragon-web.service", body)]));
        (items[0].state, items[0].detail.clone())
    }

    #[test]
    fn a_running_enabled_unit_is_ok() {
        assert_eq!(state_of(RUNNING).0, State::Ok);
    }

    #[test]
    fn failed_and_inactive_while_enabled_are_down() {
        assert_eq!(
            state_of(
                &RUNNING
                    .replace("active\n", "failed\n")
                    .replace("success", "exit-code")
            )
            .0,
            State::Down
        );
        assert_eq!(
            state_of(&RUNNING.replace("ActiveState=active", "ActiveState=inactive")).0,
            State::Down
        );
    }

    #[test]
    fn restarts_make_a_unit_degraded() {
        let (state, detail) = state_of(&RUNNING.replace("NRestarts=0", "NRestarts=2"));
        assert_eq!(state, State::Degraded);
        assert!(detail.contains("2 times"));
    }

    #[test]
    fn disabled_and_missing_units_are_off() {
        assert_eq!(
            state_of(&RUNNING.replace("=enabled", "=disabled")).0,
            State::Off
        );
        assert_eq!(
            state_of("LoadState=not-found\nActiveState=inactive\nUnitFileState=\n").0,
            State::Off
        );
    }

    #[test]
    fn a_timer_whose_last_run_failed_is_degraded() {
        let body = RUNNING.replace("success", "exit-code");
        let items = probe(&host(&[("ragon-remote-certificate.timer", &body)]));
        let timer = items.iter().find(|i| i.name.ends_with(".timer")).unwrap();
        assert_eq!(timer.state, State::Degraded);
    }

    #[test]
    fn no_systemctl_is_unknown_for_every_unit() {
        let items = probe(&FakeHost::default());
        assert_eq!(items.len(), UNITS.len());
        assert!(items.iter().all(|i| i.state == State::Unknown));
    }
}
