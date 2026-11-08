//! Machine state (§3 rule 8): MemAvailable, memory pressure, the server's RSS and, where
//! `nvidia-smi` answers, SM clock, temperature, throttle reasons and the power limit, once a
//! second on a thread. The sources are paths and a command given by the caller, so tests point them
//! at files of their own and never read the real machine.
use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct Sources {
    pub meminfo: Option<PathBuf>,
    pub psi_memory: Option<PathBuf>,
    pub server_status: Option<PathBuf>,
    /// A command printing `clocks.sm,temperature.gpu,clocks_throttle_reasons.active,power.limit,driver_version` as CSV.
    pub gpu_cmd: Option<Vec<String>>,
    pub interval: Duration,
}

impl Sources {
    pub fn none() -> Sources {
        Sources {
            meminfo: None,
            psi_memory: None,
            server_status: None,
            gpu_cmd: None,
            interval: Duration::from_secs(1),
        }
    }
    pub fn host(server_pid: Option<u32>) -> Sources {
        Sources {
            meminfo: Some("/proc/meminfo".into()),
            psi_memory: Some("/proc/pressure/memory".into()),
            server_status: server_pid.map(|p| PathBuf::from(format!("/proc/{p}/status"))),
            gpu_cmd: Some(
                ["nvidia-smi", "--query-gpu=clocks.sm,temperature.gpu,clocks_throttle_reasons.active,power.limit,driver_version", "--format=csv,noheader,nounits"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            ),
            interval: Duration::from_secs(1),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Sample {
    pub t_s: f64,
    pub mem_available_kb: Option<u64>,
    pub psi_some_avg10: Option<f64>,
    pub rss_kb: Option<u64>,
    pub sm_clock_mhz: Option<f64>,
    pub temp_c: Option<f64>,
    pub throttle_mask: Option<u64>,
    pub power_limit_w: Option<f64>,
    pub driver: Option<String>,
}

/// Throttle-reason bits that mean a thermal or power brake: HW slowdown (0x8), SW thermal (0x20),
/// HW thermal (0x40), HW power brake (0x80).
pub const THROTTLE_BITS: u64 = 0x8 | 0x20 | 0x40 | 0x80;

fn field_kb(text: &str, key: &str) -> Option<u64> {
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|r| r.split_whitespace().next()?.parse().ok())
}

pub fn read_sample(src: &Sources, t_s: f64) -> Sample {
    let mut s = Sample {
        t_s,
        ..Default::default()
    };
    let read = |p: &Option<PathBuf>| p.as_ref().and_then(|p| std::fs::read_to_string(p).ok());
    if let Some(t) = read(&src.meminfo) {
        s.mem_available_kb = field_kb(&t, "MemAvailable:");
    }
    if let Some(t) = read(&src.psi_memory) {
        s.psi_some_avg10 = t.lines().find(|l| l.starts_with("some")).and_then(|l| {
            l.split_whitespace()
                .find_map(|w| w.strip_prefix("avg10=")?.parse().ok())
        });
    }
    if let Some(t) = read(&src.server_status) {
        s.rss_kb = field_kb(&t, "VmRSS:");
    }
    if let Some(cmd) = &src.gpu_cmd
        && let Ok(out) = Command::new(&cmd[0]).args(&cmd[1..]).output()
        && out.status.success()
    {
        let text = String::from_utf8_lossy(&out.stdout);
        let cols: Vec<&str> = text
            .lines()
            .next()
            .unwrap_or("")
            .split(',')
            .map(str::trim)
            .collect();
        let num = |i: usize| cols.get(i).and_then(|c| c.parse::<f64>().ok());
        s.sm_clock_mhz = num(0);
        s.temp_c = num(1);
        s.throttle_mask = cols
            .get(2)
            .and_then(|c| u64::from_str_radix(c.trim_start_matches("0x"), 16).ok());
        s.power_limit_w = num(3);
        s.driver = cols.get(4).map(|c| c.to_string()).filter(|c| !c.is_empty());
    }
    s
}

/// Samples in the background until `stop` is called.
pub struct Sampler {
    stop: Arc<AtomicBool>,
    samples: Arc<Mutex<Vec<Sample>>>,
    handle: Option<std::thread::JoinHandle<()>>,
    started: Instant,
}

impl Sampler {
    pub fn start(src: Sources) -> Sampler {
        let stop = Arc::new(AtomicBool::new(false));
        let samples = Arc::new(Mutex::new(Vec::new()));
        let started = Instant::now();
        let (st, sm) = (stop.clone(), samples.clone());
        let handle = std::thread::spawn(move || {
            loop {
                let s = read_sample(&src, started.elapsed().as_secs_f64());
                sm.lock().unwrap().push(s);
                if st.load(Ordering::Relaxed) {
                    break;
                }
                let mut slept = Duration::ZERO;
                while slept < src.interval && !st.load(Ordering::Relaxed) {
                    let step = Duration::from_millis(10).min(src.interval);
                    std::thread::sleep(step);
                    slept += step;
                }
            }
        });
        Sampler {
            stop,
            samples,
            handle: Some(handle),
            started,
        }
    }
    pub fn now_s(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }
    pub fn stop(mut self) -> Vec<Sample> {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            h.join().ok();
        }

        self.samples.lock().unwrap().clone()
    }
}

/// Summary of the samples over [from, to] seconds (None, None for the whole run).
#[derive(Clone, Debug, Default, Serialize)]
pub struct Summary {
    pub mem_available_min_kb: Option<u64>,
    pub rss_max_kb: Option<u64>,
    pub psi_max: Option<f64>,
    pub sm_clock_median_mhz: Option<f64>,
    pub temp_max_c: Option<f64>,
    pub power_limit_w: Option<f64>,
    pub driver: Option<String>,
    pub throttle_mask_seen: u64,
}

pub fn summarise(samples: &[Sample], window: Option<(f64, f64)>) -> Summary {
    let in_window = |s: &&Sample| window.is_none_or(|(a, b)| s.t_s >= a && s.t_s <= b);
    let sel: Vec<&Sample> = samples.iter().filter(in_window).collect();
    let clocks: Vec<f64> = sel.iter().filter_map(|s| s.sm_clock_mhz).collect();
    Summary {
        mem_available_min_kb: sel.iter().filter_map(|s| s.mem_available_kb).min(),
        rss_max_kb: sel.iter().filter_map(|s| s.rss_kb).max(),
        psi_max: sel
            .iter()
            .filter_map(|s| s.psi_some_avg10)
            .fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.max(x)))),
        sm_clock_median_mhz: crate::figures::median(&clocks),
        temp_max_c: sel
            .iter()
            .filter_map(|s| s.temp_c)
            .fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.max(x)))),
        power_limit_w: sel.iter().rev().find_map(|s| s.power_limit_w),
        driver: sel.iter().find_map(|s| s.driver.clone()),
        throttle_mask_seen: sel
            .iter()
            .filter_map(|s| s.throttle_mask)
            .fold(0, |a, m| a | (m & THROTTLE_BITS)),
    }
}
