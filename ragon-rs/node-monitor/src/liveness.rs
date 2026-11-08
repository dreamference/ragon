//! Liveness (§3.3, O3): is the engine still producing tokens. Decided in the spec's order: the
//! generation counter, idle, a one-token completion under O3's four conditions, then the wedge.
//! Time is a plain number of seconds the caller supplies, so tests use a fake clock.

use crate::host::Host;
use crate::server::parse_metrics;
use crate::{Config, Item, State, Weight, gate};
use serde_json::{Value, json};
use std::time::Duration;

const NAME: &str = "liveness";
/// Nothing served for this long before a probe is allowed, and at least this long between probes.
pub const PROBE_AFTER_S: u64 = 600;
/// Requests waiting and no token for this long is a wedged engine.
pub const WEDGE_S: u64 = 60;
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);
const METRICS_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Default)]
pub struct Liveness {
    last_tokens: Option<f64>,
    /// When the generation counter last rose (or was first seen).
    last_progress: u64,
    last_probe: Option<u64>,
    probe_failures: u32,
}

fn num(metrics: &serde_json::Map<String, Value>, key: &str) -> Option<f64> {
    metrics.get(key).and_then(Value::as_f64)
}

impl Liveness {
    /// Why no probe may run now, or `None` when all four O3 conditions hold.
    fn probe_refusal(
        &self,
        now: u64,
        gate_holding: bool,
        sessions: Option<f64>,
        resident: Option<f64>,
    ) -> Option<&'static str> {
        if now.saturating_sub(self.last_progress) < PROBE_AFTER_S {
            return Some("served within 10 minutes");
        }
        if self
            .last_probe
            .is_some_and(|t| now.saturating_sub(t) < PROBE_AFTER_S)
        {
            return Some("probed within 10 minutes");
        }
        if gate_holding {
            return Some("the gate holds for a run");
        }
        match (sessions, resident) {
            (Some(total), Some(used)) if used >= total => Some("every session slot is taken"),
            // Without the slot gauges nothing proves a slot is free: do not risk a user's session.
            (None, _) | (_, None) => Some("session slots unknown"),
            _ => None,
        }
    }

    pub fn sample(&mut self, host: &dyn Host, config: &Config, now: u64) -> Item {
        let item = |state, detail: &str| Item::new(NAME, state, Weight::Node, detail);
        let body = match host.http_get(config.engine_port, "/metrics", METRICS_TIMEOUT) {
            Ok(r) if r.status == 200 => r.body,
            _ => return item(State::Unknown, "/metrics unavailable"),
        };
        let m = parse_metrics(&body);
        let Some(tokens) = num(&m, "generation_tokens_total") else {
            return item(State::Unknown, "no generation counter exported");
        };
        let rose = self.last_tokens.is_none_or(|last| tokens > last);
        self.last_tokens = Some(tokens);
        if rose {
            self.last_progress = now;
            self.probe_failures = 0;
            return item(State::Ok, "generation counter rising");
        }
        let waiting =
            num(&m, "num_running_reqs").unwrap_or(0.0) + num(&m, "num_queue_reqs").unwrap_or(0.0);
        if waiting > 0.0 {
            return if now.saturating_sub(self.last_progress) >= WEDGE_S {
                item(State::Down, "requests waiting, no tokens for 60 s")
            } else {
                item(
                    State::Ok,
                    "requests waiting, counter not yet stalled for 60 s",
                )
            };
        }
        let holding = gate::read(host, config.public_port).holding;
        let refusal = self.probe_refusal(
            now,
            holding,
            num(&m, "sessions"),
            num(&m, "sessions_resident"),
        );
        if let Some(why) = refusal {
            let detail = format!("idle ({why})");
            // A failed probe stays visible until a later probe or token clears it.
            return match self.probe_failures {
                0 => item(State::Ok, &detail),
                1 => item(State::Degraded, "last one-token completion failed"),
                _ => item(State::Down, "two one-token completions failed in a row"),
            };
        }
        self.last_probe = Some(now);
        let body = json!({"messages": [{"role": "user", "content": "."}], "max_tokens": 1, "stream": false, "user": "ragon-node-monitor"});
        let ok = host
            .http_post(
                config.engine_port,
                "/v1/chat/completions",
                &body.to_string(),
                PROBE_TIMEOUT,
            )
            .is_ok_and(|r| r.status == 200);
        if ok {
            self.probe_failures = 0;
            // The completion's own token must not read as the engine "serving" for the next probe.
            self.last_progress = now;
            return item(State::Ok, "one-token completion succeeded");
        }
        self.probe_failures += 1;
        if self.probe_failures >= 2 {
            item(State::Down, "two one-token completions failed in a row")
        } else {
            item(State::Degraded, "one-token completion failed")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeHost;

    const POST: &str = "18000 /v1/chat/completions";

    fn metrics(tokens: u64, running: u64, queued: u64, sessions: u64, resident: u64) -> String {
        format!(
            "sglang:generation_tokens_total{{model_name=\"m\"}} {tokens}\nsglang:num_running_reqs {running}\nsglang:num_queue_reqs {queued}\nragon:sessions {sessions}\nragon:sessions_resident {resident}\n"
        )
    }

    fn host(m: String, gate: Option<&str>) -> FakeHost {
        let mut h = FakeHost::default()
            .answer(18000, "/metrics", 200, 5, &m)
            .post_answer(18000, "/v1/chat/completions", 200);
        if let Some(g) = gate {
            h = h.answer(8000, "/ragon-gate", 200, 5, g);
        }
        h
    }

    fn probes(h: &FakeHost) -> usize {
        h.posted.borrow().iter().filter(|p| *p == POST).count()
    }

    /// First sample at t=0, then a second 700 s later with the counter frozen.
    fn idle_after_ten_minutes(m: String, gate: Option<&str>) -> (Item, FakeHost) {
        let h = host(m, gate);
        let mut l = Liveness::default();
        l.sample(&h, &Config::default(), 0);
        let item = l.sample(&h, &Config::default(), 700);
        (item, h)
    }

    #[test]
    fn a_rising_counter_is_live_and_runs_nothing_else() {
        let mut l = Liveness::default();
        let c = Config::default();
        l.sample(&host(metrics(10, 0, 0, 2, 0), None), &c, 0);
        let h = host(metrics(20, 0, 0, 2, 0), None);
        assert_eq!(l.sample(&h, &c, 5).state, State::Ok);
        assert!(h.posted.borrow().is_empty());
    }

    #[test]
    fn requests_queued_with_a_frozen_counter_for_60_s_are_down() {
        let c = Config::default();
        let mut l = Liveness::default();
        let h = host(metrics(10, 1, 3, 2, 1), None);
        l.sample(&h, &c, 0);
        assert_eq!(l.sample(&h, &c, 30).state, State::Ok);
        let down = l.sample(&h, &c, 61);
        assert_eq!(down.state, State::Down);
        assert_eq!(down.detail, "requests waiting, no tokens for 60 s");
        assert_eq!(probes(&h), 0);
    }

    #[test]
    fn the_probe_runs_when_every_condition_holds() {
        let (item, h) = idle_after_ten_minutes(metrics(10, 0, 0, 2, 1), None);
        assert_eq!(probes(&h), 1);
        assert_eq!(item.state, State::Ok);
    }

    #[test]
    fn never_while_the_gate_holds_for_a_run() {
        let closed = r#"{"state":"closed","done":1,"total":5}"#;
        let (item, h) = idle_after_ten_minutes(metrics(10, 0, 0, 2, 1), Some(closed));
        assert_eq!(probes(&h), 0);
        assert_eq!(item.state, State::Ok);
    }

    #[test]
    fn never_while_every_resident_session_slot_is_taken() {
        let (_, h) = idle_after_ten_minutes(metrics(10, 0, 0, 2, 2), None);
        assert_eq!(probes(&h), 0);
    }

    #[test]
    fn never_within_ten_minutes_of_the_engines_last_token() {
        let h = host(metrics(10, 0, 0, 2, 0), None);
        let mut l = Liveness::default();
        let c = Config::default();
        l.sample(&h, &c, 0);
        l.sample(&h, &c, 599);
        assert_eq!(probes(&h), 0);
        l.sample(&h, &c, 600);
        assert_eq!(probes(&h), 1);
        // And at most every ten minutes: the probe was at 600.
        l.sample(&h, &c, 900);
        l.sample(&h, &c, 1199);
        assert_eq!(probes(&h), 1);
        l.sample(&h, &c, 1200);
        assert_eq!(probes(&h), 2);
    }

    #[test]
    fn one_failed_probe_is_degraded_two_in_a_row_are_down() {
        let h = FakeHost::default().answer(18000, "/metrics", 200, 5, &metrics(10, 0, 0, 2, 0));
        let c = Config::default();
        let mut l = Liveness::default();
        l.sample(&h, &c, 0);
        assert_eq!(l.sample(&h, &c, 600).state, State::Degraded);
        assert_eq!(l.sample(&h, &c, 1200).state, State::Down);
    }
}
