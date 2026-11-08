//! The model gate (§3.2): `GET /ragon-gate` on the public port. A gate holding for a live run is
//! `ok` and says so; one whose run's heartbeat went stale is `degraded`.

use crate::host::Host;
use crate::{Item, State, Weight};
use serde_json::{Value, json};
use std::time::Duration;

const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const NAME: &str = "gate";

/// What the gate said, for the item and for the liveness rule (O3).
#[derive(Clone, Debug)]
pub struct GateReading {
    /// Requests from outside a run are refused on purpose (`closed` or `paused`).
    pub holding: bool,
    pub item: Item,
}

pub fn read(host: &dyn Host, public_port: u16) -> GateReading {
    let unknown = |why: String| GateReading {
        holding: false,
        item: Item::new(NAME, State::Unknown, Weight::Item, why),
    };
    let r = match host.http_get(public_port, "/ragon-gate", PROBE_TIMEOUT) {
        Ok(r) if r.status == 200 => r,
        Ok(r) => return unknown(format!("/ragon-gate answered {}", r.status)),
        Err(e) => return unknown(format!("/ragon-gate: {}", e.kind())),
    };
    let Ok(doc) = serde_json::from_str::<Value>(&r.body) else {
        return unknown("/ragon-gate answered something that is not JSON".into());
    };
    let state = doc["state"].as_str().unwrap_or("open");
    let holding = matches!(state, "closed" | "paused");
    let (item_state, detail) = if doc.get("stale").is_some_and(|s| !s.is_null()) {
        (
            State::Degraded,
            "stale: a run's heartbeat is old, the run died holding the gate".to_string(),
        )
    } else if holding {
        let done = doc["done"].as_u64().unwrap_or(0);
        let total = doc["total"].as_u64().unwrap_or(0);
        (
            State::Ok,
            format!("holding for a SWE-bench run, {done}/{total} done"),
        )
    } else {
        (State::Ok, "open".to_string())
    };
    GateReading {
        holding,
        item: Item::new(NAME, item_state, Weight::Item, detail)
            .with_values(json!({ "state": state, "paused_until": doc["paused_until"] })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeHost;

    fn gate(body: &str) -> GateReading {
        read(
            &FakeHost::default().answer(8000, "/ragon-gate", 200, 5, body),
            8000,
        )
    }

    #[test]
    fn a_gate_holding_for_a_live_run_is_ok_and_says_so() {
        let g = gate(r#"{"state":"closed","run":"r1","done":3,"total":10}"#);
        assert!(g.holding);
        assert_eq!(g.item.state, State::Ok);
        assert!(
            g.item
                .detail
                .contains("holding for a SWE-bench run, 3/10 done")
        );
    }

    #[test]
    fn a_gate_whose_runs_heartbeat_went_stale_is_degraded() {
        let g = gate(r#"{"state":"open","stale":"r1"}"#);
        assert!(!g.holding);
        assert_eq!(g.item.state, State::Degraded);
        assert!(g.item.detail.contains("stale"));
    }

    #[test]
    fn an_open_gate_is_ok_and_no_answer_is_unknown() {
        assert_eq!(gate(r#"{"state":"open"}"#).item.state, State::Ok);
        let none = read(&FakeHost::default(), 8000);
        assert_eq!(none.item.state, State::Unknown);
    }
}
