//! Memory (§3.5): PSI, MemAvailable, swap and OOM kills, read from /proc through the host.
//! Not in Phase 1: the PSI trigger, the container's cgroup, the host-safety layer, the PSI guard.

use crate::host::Host;
use crate::{Item, State, Weight};
use serde_json::json;

const GIB: u64 = 1 << 30;

pub fn probe(host: &dyn Host) -> Vec<Item> {
    let mut items = vec![psi(host)];
    match host.read_file("/proc/meminfo") {
        Ok(text) => {
            items.push(mem_available(&text));
            items.push(swap(&text));
        }
        Err(e) => {
            for name in ["mem_available", "swap"] {
                items.push(Item::new(
                    name,
                    State::Unknown,
                    Weight::Node,
                    format!("/proc/meminfo: {}", e.kind()),
                ));
            }
        }
    }
    items.push(oom_kills(host));
    items
}

/// `(some_avg10, some_avg60, full_avg10, full_avg60)` from /proc/pressure/memory.
pub fn parse_psi(text: &str) -> Option<[f64; 4]> {
    let field = |kind: &str, key: &str| -> Option<f64> {
        let line = text.lines().find(|l| l.starts_with(kind))?;
        line.split_whitespace()
            .find_map(|w| w.strip_prefix(key)?.parse().ok())
    };
    Some([
        field("some", "avg10=")?,
        field("some", "avg60=")?,
        field("full", "avg10=")?,
        field("full", "avg60=")?,
    ])
}

fn psi(host: &dyn Host) -> Item {
    let name = "psi";
    let text = match host.read_file("/proc/pressure/memory") {
        Ok(t) => t,
        Err(e) => {
            return Item::new(
                name,
                State::Unknown,
                Weight::Node,
                format!("/proc/pressure/memory: {}", e.kind()),
            );
        }
    };
    let Some([some10, some60, full10, full60]) = parse_psi(&text) else {
        return Item::new(
            name,
            State::Unknown,
            Weight::Node,
            "/proc/pressure/memory is not PSI text",
        );
    };
    // `down` needs avg10 >= 30 for 30 s; one sample has no duration, so it counts as it holds now.
    let state = if full10 >= 30.0 {
        State::Down
    } else if full60 >= 5.0 {
        State::Degraded
    } else {
        State::Ok
    };
    let values = json!({ "some_avg10": some10, "some_avg60": some60, "full_avg10": full10, "full_avg60": full60 });
    Item::new(
        name,
        state,
        Weight::Node,
        format!("full avg10 {full10}%, avg60 {full60}%"),
    )
    .with_values(values)
}

/// A `/proc/meminfo` figure in bytes.
pub fn meminfo_bytes(text: &str, key: &str) -> Option<u64> {
    let line = text
        .lines()
        .find(|l| l.starts_with(key) && l[key.len()..].starts_with(':'))?;
    let mut parts = line[key.len() + 1..].split_whitespace();
    let n: u64 = parts.next()?.parse().ok()?;
    Some(if parts.next() == Some("kB") {
        n * 1024
    } else {
        n
    })
}

fn gib(bytes: u64) -> f64 {
    (bytes as f64 / GIB as f64 * 10.0).round() / 10.0
}

fn mem_available(text: &str) -> Item {
    let name = "mem_available";
    let Some(avail) = meminfo_bytes(text, "MemAvailable") else {
        return Item::new(
            name,
            State::Unknown,
            Weight::Node,
            "no MemAvailable in /proc/meminfo",
        );
    };
    let state = if avail < 4 * GIB {
        State::Down
    } else if avail < 8 * GIB {
        State::Degraded
    } else {
        State::Ok
    };
    Item::new(
        name,
        state,
        Weight::Node,
        format!("{} GiB available", gib(avail)),
    )
    .with_values(json!({ "bytes": avail }))
}

fn swap(text: &str) -> Item {
    let name = "swap";
    let (Some(total), Some(free)) = (
        meminfo_bytes(text, "SwapTotal"),
        meminfo_bytes(text, "SwapFree"),
    ) else {
        return Item::new(
            name,
            State::Unknown,
            Weight::Item,
            "no SwapTotal or SwapFree in /proc/meminfo",
        );
    };
    let used = total.saturating_sub(free);
    let state = if used > 8 * GIB {
        State::Degraded
    } else {
        State::Ok
    };
    Item::new(
        name,
        state,
        Weight::Item,
        format!("{} GiB of swap used", gib(used)),
    )
    .with_values(json!({ "used_bytes": used, "total_bytes": total }))
}

fn oom_kills(host: &dyn Host) -> Item {
    let name = "oom_kills";
    let text = match host.read_file("/proc/vmstat") {
        Ok(t) => t,
        Err(e) => {
            return Item::new(
                name,
                State::Unknown,
                Weight::Item,
                format!("/proc/vmstat: {}", e.kind()),
            );
        }
    };
    let count = text
        .lines()
        .find_map(|l| l.strip_prefix("oom_kill ")?.trim().parse::<u64>().ok());
    // "Any increase" needs a previous sample (Phase 2); here the total since boot is shown.
    match count {
        Some(n) => Item::new(name, State::Ok, Weight::Item, format!("{n} since boot"))
            .with_values(json!({ "total": n })),
        None => Item::new(
            name,
            State::Unknown,
            Weight::Item,
            "no oom_kill in /proc/vmstat",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeHost;

    fn psi_text(full10: f64, full60: f64) -> String {
        format!(
            "some avg10=1.00 avg60=2.00 avg300=0.50 total=100\nfull avg10={full10:.2} avg60={full60:.2} avg300=0.00 total=50\n"
        )
    }

    fn meminfo(avail_gib: u64, swap_used_gib: u64) -> String {
        let kb = |g: u64| g * 1024 * 1024;
        format!(
            "MemTotal: {} kB\nMemAvailable: {} kB\nSwapTotal: {} kB\nSwapFree: {} kB\n",
            kb(120),
            kb(avail_gib),
            kb(16),
            kb(16 - swap_used_gib)
        )
    }

    fn host(full10: f64, full60: f64, avail: u64, swap: u64) -> FakeHost {
        FakeHost::default()
            .file("/proc/pressure/memory", &psi_text(full10, full60))
            .file("/proc/meminfo", &meminfo(avail, swap))
            .file("/proc/vmstat", "nr_free_pages 5\noom_kill 3\n")
    }

    fn states(h: &FakeHost) -> Vec<(String, State)> {
        probe(h).into_iter().map(|i| (i.name, i.state)).collect()
    }

    #[test]
    fn a_quiet_host_is_ok() {
        let s = states(&host(0.0, 0.0, 60, 0));
        assert!(s.iter().all(|(_, st)| *st == State::Ok), "{s:?}");
        assert_eq!(s.len(), 4);
    }

    #[test]
    fn psi_thresholds() {
        let state = |f10, f60| probe(&host(f10, f60, 60, 0))[0].state;
        assert_eq!(state(0.0, 4.99), State::Ok);
        assert_eq!(state(0.0, 5.0), State::Degraded);
        assert_eq!(state(29.9, 0.0), State::Ok);
        assert_eq!(state(30.0, 0.0), State::Down);
    }

    #[test]
    fn mem_available_thresholds_in_gib() {
        let state = |g| probe(&host(0.0, 0.0, g, 0))[1].state;
        assert_eq!(state(8), State::Ok);
        assert_eq!(state(7), State::Degraded);
        assert_eq!(state(4), State::Degraded);
        assert_eq!(state(3), State::Down);
    }

    #[test]
    fn swap_over_8_gib_is_degraded_and_never_down() {
        let state = |g| probe(&host(0.0, 0.0, 60, g))[2].state;
        assert_eq!(state(8), State::Ok);
        assert_eq!(state(9), State::Degraded);
    }

    #[test]
    fn missing_files_are_unknown_not_errors() {
        let items = probe(&FakeHost::default());
        assert_eq!(items.len(), 4);
        assert!(items.iter().all(|i| i.state == State::Unknown), "{items:?}");
    }

    #[test]
    fn oom_kills_are_shown_with_the_total() {
        let item = probe(&host(0.0, 0.0, 60, 0)).pop().unwrap();
        assert_eq!(
            (item.name.as_str(), item.values["total"].as_u64()),
            ("oom_kills", Some(3))
        );
    }

    #[test]
    fn meminfo_keys_match_whole_names() {
        assert_eq!(
            meminfo_bytes("SwapTotalX: 5 kB\nSwapTotal: 2 kB\n", "SwapTotal"),
            Some(2048)
        );
        assert_eq!(parse_psi("garbage"), None);
    }
}
