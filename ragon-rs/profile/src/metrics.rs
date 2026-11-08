//! `/metrics` (Prometheus text) and `/spec_stats` (JSON) snapshots, and their deltas.
use std::collections::BTreeMap;

/// Every sample of a name summed over its label sets, so `x{a="1"} 2` and `x{a="2"} 3` give 5.
#[derive(Clone, Debug, Default)]
pub struct Metrics(pub BTreeMap<String, f64>);

impl Metrics {
    pub fn parse(text: &str) -> Metrics {
        let mut m = BTreeMap::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (head, value) = match line.rsplit_once(char::is_whitespace) {
                Some(x) => x,
                None => continue,
            };
            let name = head.split('{').next().unwrap_or(head).trim();
            if let Ok(v) = value.parse::<f64>() {
                *m.entry(name.to_string()).or_insert(0.0) += v;
            }
        }
        Metrics(m)
    }
    pub fn get(&self, name: &str) -> f64 {
        self.0.get(name).copied().unwrap_or(0.0)
    }
    pub fn has(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }
    /// The first of several names that the server exports (SGLang and ragon-serve share the `sglang:` set).
    pub fn first(&self, names: &[&str]) -> f64 {
        names
            .iter()
            .find(|n| self.has(n))
            .map(|n| self.get(n))
            .unwrap_or(0.0)
    }
    pub fn delta(&self, before: &Metrics, name: &str) -> f64 {
        self.get(name) - before.get(name)
    }
}

pub const PROMPT: &str = "sglang:prompt_tokens_total";
pub const CACHED: &str = "sglang:cached_tokens_total";
pub const GENERATED: &str = "sglang:generation_tokens_total";
pub const VERIFY: &str = "sglang:spec_verify_calls_total";
pub const E2E_SUM: &str = "sglang:e2e_request_latency_seconds_sum";
pub const E2E_COUNT: &str = "sglang:e2e_request_latency_seconds_count";
pub const TTFT_SUM: &str = "sglang:time_to_first_token_seconds_sum";
pub const TTFT_COUNT: &str = "sglang:time_to_first_token_seconds_count";
pub const RUNNING: &str = "sglang:num_running_reqs";
pub const QUEUED: &str = "sglang:num_queue_reqs";
pub const DRAFTED: &str = "ragon:spec_drafted_tokens_total";
pub const ACCEPTED: &str = "ragon:spec_accepted_tokens_total";

/// Per-drafter totals from `/spec_stats`: `{"drafters": {"name": {"requests":…, "steps":…, "drafted":…,
/// "accepted":…, "draft_seconds":…, "commit_seconds":…}}}` plus top-level step counters.
#[derive(Clone, Debug, Default)]
pub struct SpecStats {
    pub drafters: BTreeMap<String, BTreeMap<String, f64>>,
    pub top: BTreeMap<String, f64>,
}

impl SpecStats {
    pub fn parse(text: &str) -> SpecStats {
        let mut s = SpecStats::default();
        let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
            return s;
        };
        let Some(obj) = v.as_object() else { return s };
        for (k, val) in obj {
            if let Some(n) = val.as_f64() {
                s.top.insert(k.clone(), n);
            } else if (k == "drafters" || k == "per_drafter")
                && let Some(d) = val.as_object()
            {
                for (name, fields) in d {
                    let f = fields
                        .as_object()
                        .map(|o| {
                            o.iter()
                                .filter_map(|(a, b)| b.as_f64().map(|n| (a.clone(), n)))
                                .collect()
                        })
                        .unwrap_or_default();
                    s.drafters.insert(name.clone(), f);
                }
            }
        }
        s
    }
    /// Sum of a drafter field over all drafters.
    pub fn total(&self, field: &str) -> f64 {
        self.drafters
            .values()
            .map(|d| d.get(field).copied().unwrap_or(0.0))
            .sum()
    }
}
