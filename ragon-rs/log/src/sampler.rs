//! The writer's own rate limit (§6.2), applied in every sink.
//!
//! Three parts: per message (10 a second), per process (50 a second at `info`, 500 at `debug` and
//! `trace`) and what always passes (the first `ERROR` of each key each second; a request's closing
//! line within its own budget of 200 a second, summarised above that every 10 seconds).

use crate::level::Level;
use std::collections::HashMap;

const PER_KEY: u32 = 10;
const REQUEST_BUDGET: u32 = 200;
const SUMMARY_EVERY_MS: u64 = 10_000;

/// What a request's closing line says, for the summary line.
#[derive(Clone, Copy, Debug)]
pub struct Request {
    pub ms: f64,
    pub error: bool,
}

/// The decision for one record.
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Write,
    Drop,
}

/// A line the sampler asks the writer to add.
#[derive(Debug, PartialEq)]
pub enum Synthetic {
    /// `WARN log lines suppressed site="<message>" count=N`
    Suppressed { site: String, count: u64 },
    /// `WARN log lines dropped count=N reason=rate`
    Dropped { count: u64 },
    /// `INFO requests summary count=… errors=… p50_ms=… p95_ms=…`
    Summary { count: u64, errors: u64, p50_ms: f64, p95_ms: f64 },
}

/// The sampler's state. Time is the caller's: milliseconds since the Unix epoch.
pub struct Sampler {
    process_cap: u32,
    second: u64,
    per_key: HashMap<(Level, String), u32>,
    suppressed: HashMap<String, u64>,
    error_keys: std::collections::HashSet<(Level, String)>,
    process_count: u32,
    dropped: u64,
    requests_this_second: u32,
    summary_since: u64,
    summarised: Vec<Request>,
}

impl Sampler {
    /// A sampler for a process at `level`: the cap is 50 at `info` and above, 500 at `debug`/`trace`.
    pub fn new(level: Level) -> Self {
        Sampler {
            process_cap: if level >= Level::Debug { 500 } else { 50 },
            second: 0,
            per_key: HashMap::new(),
            suppressed: HashMap::new(),
            error_keys: Default::default(),
            process_count: 0,
            dropped: 0,
            requests_this_second: 0,
            summary_since: 0,
            summarised: Vec::new(),
        }
    }

    /// Advances to the second of `now_ms`, returning the lines the finished second and the 10-second
    /// summary owe.
    pub fn tick(&mut self, now_ms: u64) -> Vec<Synthetic> {
        let second = now_ms / 1000;
        let mut out = Vec::new();
        if self.summary_since == 0 {
            self.summary_since = now_ms;
        }
        if second != self.second {
            let mut sites: Vec<_> = self.suppressed.drain().collect();
            sites.sort();
            out.extend(sites.into_iter().map(|(site, count)| Synthetic::Suppressed { site, count }));
            if self.dropped > 0 {
                out.push(Synthetic::Dropped { count: self.dropped });
                self.dropped = 0;
            }
            self.per_key.clear();
            self.error_keys.clear();
            self.process_count = 0;
            self.requests_this_second = 0;
            self.second = second;
        }
        if now_ms >= self.summary_since + SUMMARY_EVERY_MS {
            if !self.summarised.is_empty() {
                let mut ms: Vec<f64> = self.summarised.iter().map(|r| r.ms).collect();
                ms.sort_by(f64::total_cmp);
                let pick = |q: f64| ms[((ms.len() - 1) as f64 * q).round() as usize];
                out.push(Synthetic::Summary {
                    count: ms.len() as u64,
                    errors: self.summarised.iter().filter(|r| r.error).count() as u64,
                    p50_ms: pick(0.5),
                    p95_ms: pick(0.95),
                });
                self.summarised.clear();
            }
            self.summary_since = now_ms;
        }
        out
    }

    /// Decides one record. `request` is set on a request's closing line.
    pub fn admit(&mut self, now_ms: u64, level: Level, message: &str, request: Option<Request>) -> Decision {
        if let Some(r) = request {
            if self.requests_this_second < REQUEST_BUDGET {
                self.requests_this_second += 1;
                return Decision::Write;
            }
            self.summarised.push(r);
            return Decision::Drop;
        }
        let _ = now_ms;
        let key = (level, message.to_string());
        let first_error = level == Level::Error && self.error_keys.insert(key.clone());
        let count = self.per_key.entry(key).or_insert(0);
        *count += 1;
        if *count > PER_KEY && !first_error {
            *self.suppressed.entry(message.to_string()).or_insert(0) += 1;
            return Decision::Drop;
        }
        self.process_count += 1;
        if self.process_count > self.process_cap && !first_error {
            self.dropped += 1;
            return Decision::Drop;
        }
        Decision::Write
    }
}
