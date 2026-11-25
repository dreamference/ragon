//! Host safety on a Mac node's unified memory (specs/DREAMFERENCE_MAC_NODE.md §4.3, §9's "host
//! safety"): the pre-flight that refuses an unsafe load before it starts, and the watchdog that
//! kills the engine, and nothing else, when memory runs out during a load or while serving.
//!
//! The pre-flight is the twin of the Python node's `check_host_safety()`: every check runs and the
//! refusals are reported together, and a reading that cannot be taken refuses (every check that
//! failed open on the GB10 cost a hard reset). The plan reserves the sidecars, the twin of
//! `side_service_allowance()`. Ragon only ever READS `iogpu.wired_limit_mb`: there is no write path
//! in this module, and a refusal over it tells the owner how to return it to macOS's default, never
//! to raise it.
//!
//! The watchdog is the twin of the Linux PSI watchdog: the memory-pressure dispatch source
//! (`DISPATCH_SOURCE_TYPE_MEMORYPRESSURE`) plus a swap and compressor sample once a second. It
//! trips when pressure stays critical for `critical_for`, or when swap grows by more than
//! `swap_growth_during_load` during a load, and the trip goes through the supervisor's
//! `Engine::kill` (ragon-rs/supervisor/src/core.rs), which on macOS sends `SIGKILL` to the engine's
//! process group. The thresholds marked [E] are estimates until G15 measures them (§8).
//!
//! The decisions are plain Rust over the `HostReadings` trait, tested on every platform with
//! fakes; only `mac::MacHost`, the real readings, is macOS code.

use std::fmt;
use std::io;
use std::time::{Duration, Instant};

use ragon_supervisor::core::Engine;

const GIB: u64 = 1 << 30;
const MIB: u64 = 1 << 20;

/// The system's memory pressure: `kern.memorystatus_vm_pressure_level` and the dispatch source's
/// data both encode it as 1 normal, 2 warning, 4 critical.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pressure {
    Normal,
    Warning,
    Critical,
}

impl Pressure {
    pub fn from_level(level: u64) -> Self {
        if level & 4 != 0 {
            Pressure::Critical
        } else if level & 2 != 0 {
            Pressure::Warning
        } else {
            Pressure::Normal
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Pressure::Normal => "normal",
            Pressure::Warning => "warning",
            Pressure::Critical => "critical",
        }
    }
}

/// `NSProcessInfo.thermalState`'s four levels. The Mac reader takes them from the system's thermal
/// pressure notification (`com.apple.system.thermalpressurelevel`: 0 nominal, 1 moderate, 2 heavy,
/// 3 trapping, 4 sleeping), the state `thermalState` is derived from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Thermal {
    Nominal,
    Fair,
    Serious,
    Critical,
}

impl Thermal {
    pub fn from_pressure_level(level: u64) -> Self {
        match level {
            0 => Thermal::Nominal,
            1 => Thermal::Fair,
            2 => Thermal::Serious,
            _ => Thermal::Critical,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Thermal::Nominal => "nominal",
            Thermal::Fair => "fair",
            Thermal::Serious => "serious",
            Thermal::Critical => "critical",
        }
    }
}

/// What the pre-flight reads; `None` where the source could not be read.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Readings {
    /// `hw.memsize`, bytes.
    pub physical_memory: Option<u64>,
    /// Metal's `recommendedMaxWorkingSetSize`, bytes.
    pub gpu_working_set_cap: Option<u64>,
    /// `iogpu.wired_limit_mb`; 0 is macOS's default (the limit was not changed).
    pub wired_limit_mb: Option<u64>,
    /// `kern.memorystatus_vm_pressure_level`.
    pub pressure: Option<Pressure>,
    /// `vm.swapusage`'s used bytes.
    pub swap_used: Option<u64>,
    /// `vm.compressor_bytes_used`: the memory the compressor occupies.
    pub compressor_bytes: Option<u64>,
    /// The thermal state.
    pub thermal: Option<Thermal>,
    /// The GPU's memory in use by other processes (the accelerator's "In use system memory").
    pub gpu_in_use: Option<u64>,
}

/// One watchdog sample, taken once a second; it allocates nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    pub pressure: Pressure,
    pub swap_used: u64,
    pub compressor_bytes: u64,
}

/// The host's readings. Read only: the trait has no setter, so nothing here can change the system.
pub trait HostReadings {
    /// Everything the pre-flight looks at, once, before a load.
    fn readings(&mut self) -> Readings;
    /// The watchdog's once-a-second sample.
    fn sample(&mut self) -> io::Result<Sample>;
}

/// The load the pre-flight is asked about.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// `ragon-serve`'s own plan refused (exit 3, Metal spec §4.8): its reason.
    pub engine_refusal: Option<String>,
    /// The GPU-resident bytes the engine's plan needs (weights, KV, scratch).
    pub gpu_need: u64,
    /// What the recipe keeps for macOS: its `memory_gb` minus its `gpu_cap_gb`.
    pub host_reserve: u64,
    /// The capped services beside the engine, (name, bytes): `side_service_allowance()`'s twin.
    pub sidecars: Vec<(String, u64)>,
}

impl Plan {
    /// A plan over a Metal recipe: the recipe's reserve for macOS, the engine's need, the sidecars.
    pub fn for_recipe(
        recipe: &crate::metal_recipe::MetalRecipe,
        gpu_need: u64,
        sidecars: Vec<(String, u64)>,
    ) -> Self {
        Plan {
            engine_refusal: None,
            gpu_need,
            host_reserve: u64::from(recipe.memory_gb.saturating_sub(recipe.gpu_cap_gb)) * GIB,
            sidecars,
        }
    }

    pub fn sidecar_bytes(&self) -> u64 {
        self.sidecars.iter().map(|(_, bytes)| bytes).sum()
    }
}

/// The thresholds. Those marked [E] are estimates until G15 measures them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Thresholds {
    /// Headroom kept under the GPU working-set cap for the window server and the system [E].
    pub gpu_reserve: u64,
    /// Swap in use before a load above which the load is refused [E].
    pub swap_before_load: u64,
    /// How long pressure must stay critical before the watchdog trips (§4.3: 3 s) [E].
    pub critical_for: Duration,
    /// Swap growth during a load above which the watchdog trips [E].
    pub swap_growth_during_load: u64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds {
            gpu_reserve: 2 * GIB,
            swap_before_load: GIB,
            critical_for: Duration::from_secs(3),
            swap_growth_during_load: 2 * GIB,
        }
    }
}

/// One reason the pre-flight refuses a load (§4.3's table, one variant per row).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// A reading could not be taken; refusing is the safe answer.
    Unreadable(&'static str),
    /// The engine's own plan says no.
    EnginePlan(String),
    /// The engine, the sidecars and macOS's reserve together exceed the machine's memory.
    PlanDoesNotFit { need: u64, sidecars: u64, reserve: u64, physical: u64 },
    /// The plan needs more than the GPU working-set cap minus the reserve.
    GpuCap { need: u64, reserve: u64, cap: u64 },
    /// `iogpu.wired_limit_mb` was raised so far that macOS keeps less than the recipe's reserve.
    WiredLimitRaised { limit_mb: u64, left: u64, reserve: u64 },
    /// Memory pressure is warning or critical now.
    Pressure(Pressure),
    /// Swap in use above the threshold before the load.
    Swap { used: u64, threshold: u64 },
    /// The thermal state is critical.
    Thermal,
    /// Another process holds so much of the GPU that the plan no longer fits under the cap.
    GpuInUse { in_use: u64, need: u64, cap: u64 },
}

fn gib(bytes: u64) -> String {
    format!("{:.1} GiB", bytes as f64 / GIB as f64)
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Unreadable(what) => write!(
                f,
                "{what} could not be read, so the load cannot be shown to be safe."
            ),
            Refusal::EnginePlan(reason) => write!(f, "The engine's own plan refuses: {reason}"),
            Refusal::PlanDoesNotFit { need, sidecars, reserve, physical } => write!(
                f,
                "The engine needs {}, the sidecars {} and macOS keeps {}, more than this Mac's {}.\n   Turn a sidecar off or choose a smaller recipe.",
                gib(*need),
                gib(*sidecars),
                gib(*reserve),
                gib(*physical)
            ),
            Refusal::GpuCap { need, reserve, cap } => write!(
                f,
                "The plan needs {} on the GPU plus {} of headroom; Metal's working-set cap is {}.\n   Choose a smaller recipe (fewer sessions or a shorter context).",
                gib(*need),
                gib(*reserve),
                gib(*cap)
            ),
            Refusal::WiredLimitRaised { limit_mb, left, reserve } => write!(
                f,
                "iogpu.wired_limit_mb is {limit_mb}, which leaves macOS {}; the recipe keeps {} for it.\n   Ragon never changes this setting. To return it to macOS's default:\n     sudo sysctl iogpu.wired_limit_mb=0",
                gib(*left),
                gib(*reserve)
            ),
            Refusal::Pressure(level) => write!(
                f,
                "Memory pressure is {} now; a load would push the user's applications into swap.\n   Close memory-heavy applications and start again.",
                level.as_str()
            ),
            Refusal::Swap { used, threshold } => write!(
                f,
                "{} of swap is in use, above the {} a load may start with.\n   Close memory-heavy applications (or restart the Mac) and start again.",
                gib(*used),
                gib(*threshold)
            ),
            Refusal::Thermal => write!(
                f,
                "The Mac's thermal state is critical. Let it cool and start again."
            ),
            Refusal::GpuInUse { in_use, need, cap } => write!(
                f,
                "Other processes hold {} of GPU memory; with the plan's {} that passes the {} cap.\n   Quit the other GPU application and start again.",
                gib(*in_use),
                gib(*need),
                gib(*cap)
            ),
        }
    }
}

/// The pre-flight: every row of §4.3's table, all refusals at once. `Ok(())` means load.
pub fn preflight(plan: &Plan, readings: &Readings, t: &Thresholds) -> Result<(), Vec<Refusal>> {
    let mut refusals = Vec::new();

    if let Some(reason) = &plan.engine_refusal {
        refusals.push(Refusal::EnginePlan(reason.clone()));
    }

    let sidecars = plan.sidecar_bytes();
    match readings.physical_memory {
        None => refusals.push(Refusal::Unreadable("The machine's memory (hw.memsize)")),
        Some(physical) => {
            if plan.gpu_need.saturating_add(sidecars).saturating_add(plan.host_reserve) > physical {
                refusals.push(Refusal::PlanDoesNotFit {
                    need: plan.gpu_need,
                    sidecars,
                    reserve: plan.host_reserve,
                    physical,
                });
            }
        }
    }

    match (readings.wired_limit_mb, readings.physical_memory) {
        (None, _) => refusals.push(Refusal::Unreadable("iogpu.wired_limit_mb")),
        // 0 is macOS's own default: the limit was not raised. Without hw.memsize the row above
        // has already refused.
        (Some(0), _) | (Some(_), None) => {}
        (Some(limit_mb), Some(physical)) => {
            let left = physical.saturating_sub(limit_mb.saturating_mul(MIB));
            let reserve = plan.host_reserve.saturating_add(sidecars);
            if left < reserve {
                refusals.push(Refusal::WiredLimitRaised { limit_mb, left, reserve });
            }
        }
    }

    match readings.gpu_working_set_cap {
        None => refusals.push(Refusal::Unreadable("Metal's recommendedMaxWorkingSetSize")),
        Some(cap) => {
            let need = plan.gpu_need.saturating_add(t.gpu_reserve);
            if need > cap {
                refusals.push(Refusal::GpuCap { need: plan.gpu_need, reserve: t.gpu_reserve, cap });
            } else {
                match readings.gpu_in_use {
                    None => refusals.push(Refusal::Unreadable("The GPU's memory in use")),
                    Some(in_use) if in_use.saturating_add(need) > cap => {
                        refusals.push(Refusal::GpuInUse { in_use, need: plan.gpu_need, cap })
                    }
                    Some(_) => {}
                }
            }
        }
    }

    match readings.pressure {
        None => refusals.push(Refusal::Unreadable("kern.memorystatus_vm_pressure_level")),
        Some(Pressure::Normal) => {}
        Some(level) => refusals.push(Refusal::Pressure(level)),
    }

    match readings.swap_used {
        None => refusals.push(Refusal::Unreadable("vm.swapusage")),
        Some(used) if used > t.swap_before_load => {
            refusals.push(Refusal::Swap { used, threshold: t.swap_before_load })
        }
        Some(_) => {}
    }

    match readings.thermal {
        None => refusals.push(Refusal::Unreadable("The thermal state")),
        Some(Thermal::Critical) => refusals.push(Refusal::Thermal),
        Some(_) => {}
    }

    if refusals.is_empty() { Ok(()) } else { Err(refusals) }
}

/// The refusal message, numbered as `check_host_safety()` numbers its problems.
pub fn refusal_text(refusals: &[Refusal]) -> String {
    let listed: Vec<String> =
        refusals.iter().enumerate().map(|(i, r)| format!("{}. {r}", i + 1)).collect();
    format!(
        "Refusing to load: {} host safety {} failed.\n\n{}\n",
        refusals.len(),
        if refusals.len() == 1 { "check" } else { "checks" },
        listed.join("\n\n")
    )
}

/// Why the watchdog tripped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trip {
    /// Pressure was critical for at least `critical_for`.
    PressureCritical { lasted: Duration },
    /// Swap grew by more than the threshold during a load.
    SwapGrowth { grew: u64, threshold: u64, compressor_bytes: u64 },
}

impl fmt::Display for Trip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Trip::PressureCritical { lasted } => write!(
                f,
                "memory pressure critical for {:.1} s: the engine was killed",
                lasted.as_secs_f64()
            ),
            Trip::SwapGrowth { grew, threshold, compressor_bytes } => write!(
                f,
                "swap grew {} during the load (limit {}; compressor {}): the engine was killed",
                gib(*grew),
                gib(*threshold),
                gib(*compressor_bytes)
            ),
        }
    }
}

/// A trip and the outcome of the kill it sent.
#[derive(Debug)]
pub struct Tripped {
    pub trip: Trip,
    pub kill: io::Result<()>,
}

/// The watchdog's state between samples. Fixed size: observing a sample allocates nothing.
#[derive(Debug, Clone)]
pub struct Watchdog {
    thresholds: Thresholds,
    critical_since: Option<Instant>,
    load_swap_base: Option<u64>,
}

impl Watchdog {
    pub fn new(thresholds: Thresholds) -> Self {
        Watchdog { thresholds, critical_since: None, load_swap_base: None }
    }

    /// A load begins: swap growth from here on counts.
    pub fn load_started(&mut self, swap_used: u64) {
        self.load_swap_base = Some(swap_used);
    }

    /// The load is done (the engine answers): swap growth no longer trips, pressure still does.
    pub fn load_finished(&mut self) {
        self.load_swap_base = None;
    }

    pub fn loading(&self) -> bool {
        self.load_swap_base.is_some()
    }

    /// Decides on one sample. A trip resets the state, so the restarted engine starts clean.
    pub fn observe(&mut self, now: Instant, sample: &Sample) -> Option<Trip> {
        let trip = if sample.pressure == Pressure::Critical {
            let since = *self.critical_since.get_or_insert(now);
            let lasted = now.saturating_duration_since(since);
            (lasted >= self.thresholds.critical_for).then_some(Trip::PressureCritical { lasted })
        } else {
            self.critical_since = None;
            None
        };
        let trip = trip.or_else(|| {
            let base = self.load_swap_base?;
            let grew = sample.swap_used.saturating_sub(base);
            (grew > self.thresholds.swap_growth_during_load).then_some(Trip::SwapGrowth {
                grew,
                threshold: self.thresholds.swap_growth_during_load,
                compressor_bytes: sample.compressor_bytes,
            })
        });
        if trip.is_some() {
            self.critical_since = None;
            self.load_swap_base = None;
        }
        trip
    }

    /// One watchdog step: on a trip, kill the engine through the supervisor's `Engine::kill` and do
    /// nothing else (no other process, no setting, no restart: restarting is the supervisor's).
    pub fn tick<E: Engine + ?Sized>(
        &mut self,
        engine: &mut E,
        now: Instant,
        sample: &Sample,
    ) -> Option<Tripped> {
        let trip = self.observe(now, sample)?;
        Some(Tripped { trip, kill: engine.kill() })
    }
}

/// The GPU memory in use from `ioreg -r -c AGXAccelerator -d 1`: the largest
/// `"In use system memory"=N` (one per accelerator).
pub fn parse_gpu_in_use(ioreg: &str) -> Option<u64> {
    const KEY: &str = "\"In use system memory\"=";
    ioreg
        .match_indices(KEY)
        .filter_map(|(at, _)| {
            let digits: String = ioreg[at + KEY.len()..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            digits.parse().ok()
        })
        .max()
}

/// The real readings (macOS only). Every system call here reads; none writes.
#[cfg(target_os = "macos")]
pub mod mac {
    use super::{HostReadings, Pressure, Readings, Sample, Thermal};
    use std::ffi::{CStr, c_char, c_int, c_void};
    use std::io;
    use std::sync::Once;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Reads a sysctl into `buf`. `newp` is null and `newlen` 0, so the call cannot change it.
    fn sysctl_read(name: &CStr, buf: &mut [u8]) -> io::Result<usize> {
        let mut len = buf.len();
        let rc = unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                buf.as_mut_ptr().cast(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(len)
    }

    /// An integer sysctl (4 or 8 bytes).
    pub fn sysctl_u64(name: &CStr) -> io::Result<u64> {
        let mut buf = [0u8; 8];
        match sysctl_read(name, &mut buf)? {
            4 => Ok(u64::from(u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]))),
            8 => Ok(u64::from_ne_bytes(buf)),
            n => Err(io::Error::new(io::ErrorKind::InvalidData, format!("{n}-byte value"))),
        }
    }

    /// `vm.swapusage`'s used bytes: `struct xsw_usage { total, avail, used: u64; pagesize: u32; encrypted }`.
    pub fn swap_used() -> io::Result<u64> {
        let mut buf = [0u8; 32];
        if sysctl_read(c"vm.swapusage", &mut buf)? < 24 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "short xsw_usage"));
        }
        let mut used = [0u8; 8];
        used.copy_from_slice(&buf[16..24]);
        Ok(u64::from_ne_bytes(used))
    }

    #[repr(C)]
    struct DispatchSourceType {
        _opaque: [u8; 0],
    }

    unsafe extern "C" {
        static _dispatch_source_type_memorypressure: DispatchSourceType;
        fn dispatch_get_global_queue(identifier: isize, flags: usize) -> *mut c_void;
        fn dispatch_source_create(
            kind: *const DispatchSourceType,
            handle: usize,
            mask: usize,
            queue: *mut c_void,
        ) -> *mut c_void;
        fn dispatch_set_context(object: *mut c_void, context: *mut c_void);
        fn dispatch_source_set_event_handler_f(source: *mut c_void, handler: extern "C" fn(*mut c_void));
        fn dispatch_source_get_data(source: *mut c_void) -> usize;
        fn dispatch_resume(object: *mut c_void);

        fn notify_register_check(name: *const c_char, token: *mut c_int) -> u32;
        fn notify_get_state(token: c_int, state: *mut u64) -> u32;
        fn notify_cancel(token: c_int) -> u32;
    }

    #[link(name = "Metal", kind = "framework")]
    unsafe extern "C" {
        fn MTLCreateSystemDefaultDevice() -> *mut c_void;
    }

    #[link(name = "objc")]
    unsafe extern "C" {
        fn sel_registerName(name: *const c_char) -> *const c_void;
        fn objc_msgSend();
        fn objc_release(object: *mut c_void);
    }

    /// The last level the memory-pressure dispatch source delivered (0 until it fires).
    static SOURCE_LEVEL: AtomicU64 = AtomicU64::new(0);
    static SOURCE: Once = Once::new();

    extern "C" fn on_pressure(source: *mut c_void) {
        let level = unsafe { dispatch_source_get_data(source) } as u64;
        SOURCE_LEVEL.store(level, Ordering::Relaxed);
    }

    /// Subscribes to `DISPATCH_SOURCE_TYPE_MEMORYPRESSURE` (normal, warning, critical) once per
    /// process; the source lives as long as the process.
    fn start_pressure_source() {
        SOURCE.call_once(|| unsafe {
            let queue = dispatch_get_global_queue(0, 0);
            let source = dispatch_source_create(
                std::ptr::addr_of!(_dispatch_source_type_memorypressure),
                0,
                0x1 | 0x2 | 0x4,
                queue,
            );
            if source.is_null() {
                return;
            }
            dispatch_set_context(source, source);
            dispatch_source_set_event_handler_f(source, on_pressure);
            dispatch_resume(source);
        });
    }

    /// Metal's `recommendedMaxWorkingSetSize` of the default device.
    pub fn gpu_working_set_cap() -> Option<u64> {
        unsafe {
            let device = MTLCreateSystemDefaultDevice();
            if device.is_null() {
                return None;
            }
            let sel = sel_registerName(c"recommendedMaxWorkingSetSize".as_ptr());
            let send: extern "C" fn(*mut c_void, *const c_void) -> u64 =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            let cap = send(device, sel);
            objc_release(device);
            Some(cap)
        }
    }

    /// The thermal state from the system's thermal pressure notification.
    pub fn thermal() -> Option<Thermal> {
        unsafe {
            let mut token: c_int = 0;
            if notify_register_check(c"com.apple.system.thermalpressurelevel".as_ptr(), &mut token) != 0 {
                return None;
            }
            let mut state: u64 = 0;
            let ok = notify_get_state(token, &mut state) == 0;
            notify_cancel(token);
            ok.then(|| Thermal::from_pressure_level(state))
        }
    }

    /// The GPU's memory in use, from the accelerator's performance statistics.
    pub fn gpu_in_use() -> Option<u64> {
        let out = std::process::Command::new("/usr/sbin/ioreg")
            .args(["-r", "-c", "AGXAccelerator", "-d", "1"])
            .output()
            .ok()?;
        super::parse_gpu_in_use(&String::from_utf8_lossy(&out.stdout))
    }

    fn pressure_now() -> io::Result<Pressure> {
        let sysctl = Pressure::from_level(sysctl_u64(c"kern.memorystatus_vm_pressure_level")?);
        let source = Pressure::from_level(SOURCE_LEVEL.load(Ordering::Relaxed));
        Ok(sysctl.max(source))
    }

    /// The Mac's readings; `new` starts the memory-pressure source.
    pub struct MacHost(());

    impl MacHost {
        pub fn new() -> Self {
            start_pressure_source();
            MacHost(())
        }
    }

    impl Default for MacHost {
        fn default() -> Self {
            Self::new()
        }
    }

    impl HostReadings for MacHost {
        fn readings(&mut self) -> Readings {
            Readings {
                physical_memory: sysctl_u64(c"hw.memsize").ok(),
                gpu_working_set_cap: gpu_working_set_cap(),
                wired_limit_mb: sysctl_u64(c"iogpu.wired_limit_mb").ok(),
                pressure: pressure_now().ok(),
                swap_used: swap_used().ok(),
                compressor_bytes: sysctl_u64(c"vm.compressor_bytes_used").ok(),
                thermal: thermal(),
                gpu_in_use: gpu_in_use(),
            }
        }

        fn sample(&mut self) -> io::Result<Sample> {
            Ok(Sample {
                pressure: pressure_now()?,
                swap_used: swap_used()?,
                compressor_bytes: sysctl_u64(c"vm.compressor_bytes_used")?,
            })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Reads the real host once and prints it (`--nocapture`); writes nothing.
        #[test]
        fn reads_the_real_host() {
            let mut host = MacHost::new();
            let readings = host.readings();
            println!("{readings:#?}");
            println!("{:#?}", host.sample().unwrap());
            assert!(readings.physical_memory.unwrap() > 0);
            assert!(readings.wired_limit_mb.is_some());
            assert!(readings.pressure.is_some());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// The world the fakes act on: the only effect the watchdog may have is a kill of the engine.
    #[derive(Default)]
    struct World {
        kills: u32,
        starts: u32,
        sample_calls: u32,
    }

    struct FakeEngine(Rc<RefCell<World>>);

    impl Engine for FakeEngine {
        fn start(&mut self) -> io::Result<u32> {
            self.0.borrow_mut().starts += 1;
            Ok(4242)
        }
        fn try_wait(&mut self) -> io::Result<Option<i32>> {
            Ok(None)
        }
        fn kill(&mut self) -> io::Result<()> {
            self.0.borrow_mut().kills += 1;
            Ok(())
        }
    }

    struct FakeHost {
        readings: Readings,
        samples: Vec<Sample>,
        world: Rc<RefCell<World>>,
    }

    impl HostReadings for FakeHost {
        fn readings(&mut self) -> Readings {
            self.readings.clone()
        }
        fn sample(&mut self) -> io::Result<Sample> {
            let mut world = self.world.borrow_mut();
            let at = world.sample_calls as usize;
            world.sample_calls += 1;
            self.samples
                .get(at)
                .copied()
                .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "no more samples"))
        }
    }

    /// A healthy 64 GB Mac (the gate's mini): readings as the node reads them.
    fn healthy() -> Readings {
        Readings {
            physical_memory: Some(64 * GIB),
            gpu_working_set_cap: Some(48 * GIB),
            wired_limit_mb: Some(0),
            pressure: Some(Pressure::Normal),
            swap_used: Some(512 * MIB),
            compressor_bytes: Some(400 * MIB),
            thermal: Some(Thermal::Nominal),
            gpu_in_use: Some(16 * MIB),
        }
    }

    /// The 64 GB recipe (16 GB kept for macOS) with a 30 GiB engine and a 2 GiB forge.
    fn plan() -> Plan {
        let recipes = crate::metal_recipe::MetalRecipes::parse(include_str!(
            "../vectors/metal-recipes.json"
        ))
        .unwrap();
        Plan::for_recipe(
            recipes.pick(64).unwrap(),
            30 * GIB,
            vec![("the forge, ragon-forge".to_string(), 2 * GIB)],
        )
    }

    fn refused(plan: &Plan, readings: &Readings) -> Vec<Refusal> {
        preflight(plan, readings, &Thresholds::default()).unwrap_err()
    }

    #[test]
    fn a_healthy_host_loads() {
        assert_eq!(preflight(&plan(), &healthy(), &Thresholds::default()), Ok(()));
        assert_eq!(plan().host_reserve, 16 * GIB);
    }

    #[test]
    fn the_engines_own_plan_refuses() {
        let mut p = plan();
        p.engine_refusal = Some("KV for 2 sessions at 131072 tokens does not fit".into());
        assert_eq!(
            refused(&p, &healthy()),
            vec![Refusal::EnginePlan("KV for 2 sessions at 131072 tokens does not fit".into())]
        );
    }

    #[test]
    fn the_sidecars_are_reserved_in_the_plan() {
        // 46 + 2 + 16 = 64 fits exactly; a 4 GiB forge does not.
        let mut p = plan();
        p.gpu_need = 46 * GIB;
        let mut r = healthy();
        r.gpu_working_set_cap = Some(64 * GIB);
        assert_eq!(preflight(&p, &r, &Thresholds::default()), Ok(()));
        p.sidecars[0].1 = 4 * GIB;
        assert_eq!(
            refused(&p, &r),
            vec![Refusal::PlanDoesNotFit {
                need: 46 * GIB,
                sidecars: 4 * GIB,
                reserve: 16 * GIB,
                physical: 64 * GIB
            }]
        );
    }

    #[test]
    fn more_than_the_working_set_cap_minus_the_reserve_refuses() {
        let mut p = plan();
        p.gpu_need = 47 * GIB; // 47 + 2 GiB headroom > 48
        let refusals = refused(&p, &healthy());
        assert!(refusals.contains(&Refusal::GpuCap { need: 47 * GIB, reserve: 2 * GIB, cap: 48 * GIB }));
        assert!(refusals[0].to_string().contains("smaller recipe"));
    }

    #[test]
    fn a_wired_limit_raised_past_the_reserve_refuses_and_is_never_raised() {
        let mut r = healthy();
        // 56 GB wired leaves macOS 8 GiB; the recipe keeps 16 for it plus 2 for the forge.
        r.wired_limit_mb = Some(56 * 1024);
        let refusals = refused(&plan(), &r);
        assert_eq!(
            refusals,
            vec![Refusal::WiredLimitRaised { limit_mb: 56 * 1024, left: 8 * GIB, reserve: 18 * GIB }]
        );
        let text = refusals[0].to_string();
        assert!(text.contains("never changes this setting"));
        assert!(text.contains("iogpu.wired_limit_mb=0"));
        // A limit set but still leaving the reserve is the owner's choice and passes.
        r.wired_limit_mb = Some(40 * 1024);
        assert_eq!(preflight(&plan(), &r, &Thresholds::default()), Ok(()));
    }

    #[test]
    fn this_module_has_no_path_that_writes_a_sysctl() {
        // The needles are built at run time so this test's own text does not match them.
        let source = include_str!("host_safety_macos.rs");
        let call_site = ["libc::sysctl", "byname("].concat();
        assert_eq!(source.matches(&call_site).count(), 1, "one sysctl call site, the read-only one");
        let at = source.find(&call_site).unwrap();
        let call: String = source[at..at + 300].split_whitespace().collect();
        assert!(call.contains("std::ptr::null_mut(),0,"), "newp null, newlen 0: {call}");
        assert!(!source.contains(&["Command::new(\"/usr/sbin/", "sysctl"].concat()));
        assert!(!source.contains(&["sysctl", "-w"].join(" ")));
    }

    #[test]
    fn warning_or_critical_pressure_refuses() {
        for level in [Pressure::Warning, Pressure::Critical] {
            let mut r = healthy();
            r.pressure = Some(level);
            assert_eq!(refused(&plan(), &r), vec![Refusal::Pressure(level)]);
        }
    }

    #[test]
    fn swap_in_use_above_the_threshold_refuses() {
        let mut r = healthy();
        r.swap_used = Some(GIB + 1);
        assert_eq!(refused(&plan(), &r), vec![Refusal::Swap { used: GIB + 1, threshold: GIB }]);
        r.swap_used = Some(GIB);
        assert_eq!(preflight(&plan(), &r, &Thresholds::default()), Ok(()));
    }

    #[test]
    fn a_critical_thermal_state_refuses_and_serious_does_not() {
        let mut r = healthy();
        r.thermal = Some(Thermal::Serious);
        assert_eq!(preflight(&plan(), &r, &Thresholds::default()), Ok(()));
        r.thermal = Some(Thermal::Critical);
        assert_eq!(refused(&plan(), &r), vec![Refusal::Thermal]);
    }

    #[test]
    fn another_heavy_gpu_user_refuses() {
        let mut r = healthy();
        r.gpu_in_use = Some(20 * GIB); // 20 + 30 + 2 > 48
        assert_eq!(
            refused(&plan(), &r),
            vec![Refusal::GpuInUse { in_use: 20 * GIB, need: 30 * GIB, cap: 48 * GIB }]
        );
    }

    #[test]
    fn an_unreadable_source_refuses_and_all_refusals_come_together() {
        let r = Readings::default();
        let refusals = refused(&plan(), &r);
        assert_eq!(refusals.len(), 6, "{refusals:?}");
        assert!(refusals.iter().all(|x| matches!(x, Refusal::Unreadable(_))));
        let text = refusal_text(&refusals);
        assert!(text.starts_with("Refusing to load: 6 host safety checks failed."));
        assert!(text.contains("\n\n6. "));
    }

    #[test]
    fn levels_decode_as_the_kernel_writes_them() {
        assert_eq!(Pressure::from_level(1), Pressure::Normal);
        assert_eq!(Pressure::from_level(2), Pressure::Warning);
        assert_eq!(Pressure::from_level(4), Pressure::Critical);
        assert_eq!(Pressure::from_level(0), Pressure::Normal);
        assert_eq!(Thermal::from_pressure_level(0), Thermal::Nominal);
        assert_eq!(Thermal::from_pressure_level(2), Thermal::Serious);
        assert_eq!(Thermal::from_pressure_level(3), Thermal::Critical);
        assert_eq!(Thermal::from_pressure_level(4), Thermal::Critical);
    }

    #[test]
    fn the_gpu_in_use_is_read_from_the_accelerator() {
        let ioreg = r#"  | |   "PerformanceStatistics" = {"In use system memory (driver)"=0,"Alloc system memory"=652378112,"In use system memory"=15368192}"#;
        assert_eq!(parse_gpu_in_use(ioreg), Some(15368192));
        assert_eq!(parse_gpu_in_use("no accelerator"), None);
    }

    fn sample(pressure: Pressure, swap_used: u64) -> Sample {
        Sample { pressure, swap_used, compressor_bytes: 400 * MIB }
    }

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    /// Runs the watchdog over the fake host's samples, one a second, as the supervisor will.
    fn run(host: &mut FakeHost, dog: &mut Watchdog, engine: &mut FakeEngine) -> Vec<(u64, Trip)> {
        let t0 = Instant::now();
        let mut trips = Vec::new();
        let mut second = 0;
        while let Ok(s) = host.sample() {
            if let Some(tripped) = dog.tick(engine, t0 + secs(second), &s) {
                tripped.kill.unwrap();
                trips.push((second, tripped.trip));
            }
            second += 1;
        }
        trips
    }

    fn rig(samples: Vec<Sample>) -> (Rc<RefCell<World>>, FakeHost, FakeEngine, Watchdog) {
        let world = Rc::new(RefCell::new(World::default()));
        let host = FakeHost { readings: healthy(), samples, world: world.clone() };
        (world.clone(), host, FakeEngine(world), Watchdog::new(Thresholds::default()))
    }

    #[test]
    fn pressure_critical_for_three_seconds_trips_the_kill_and_nothing_else() {
        let c = Pressure::Critical;
        let (world, mut host, mut engine, mut dog) =
            rig(vec![sample(c, 0), sample(c, 0), sample(c, 0), sample(c, 0)]);
        let trips = run(&mut host, &mut dog, &mut engine);
        assert_eq!(trips, vec![(3, Trip::PressureCritical { lasted: secs(3) })]);
        let world = world.borrow();
        assert_eq!((world.kills, world.starts), (1, 0), "one kill, no restart, nothing else");
    }

    #[test]
    fn a_short_critical_spell_or_a_warning_does_not_trip() {
        let (c, w, n) = (Pressure::Critical, Pressure::Warning, Pressure::Normal);
        let (world, mut host, mut engine, mut dog) = rig(vec![
            sample(c, 0),
            sample(c, 0),
            sample(w, 0),
            sample(c, 0),
            sample(c, 0),
            sample(n, 0),
            sample(w, 0),
            sample(w, 0),
            sample(w, 0),
            sample(w, 0),
        ]);
        assert!(run(&mut host, &mut dog, &mut engine).is_empty());
        assert_eq!(world.borrow().kills, 0);
    }

    #[test]
    fn swap_growing_past_the_threshold_during_a_load_trips() {
        let n = Pressure::Normal;
        let (world, mut host, mut engine, mut dog) =
            rig(vec![sample(n, GIB), sample(n, 2 * GIB), sample(n, 3 * GIB + 1)]);
        dog.load_started(GIB);
        let trips = run(&mut host, &mut dog, &mut engine);
        assert_eq!(
            trips,
            vec![(2, Trip::SwapGrowth { grew: 2 * GIB + 1, threshold: 2 * GIB, compressor_bytes: 400 * MIB })]
        );
        assert!(trips[0].1.to_string().contains("the engine was killed"));
        assert_eq!((world.borrow().kills, world.borrow().starts), (1, 0));
        assert!(!dog.loading(), "a trip ends the load's accounting");
    }

    #[test]
    fn swap_growth_outside_a_load_does_not_trip() {
        let n = Pressure::Normal;
        let (world, mut host, mut engine, mut dog) =
            rig(vec![sample(n, GIB), sample(n, 8 * GIB)]);
        dog.load_started(GIB);
        dog.load_finished();
        assert!(run(&mut host, &mut dog, &mut engine).is_empty());
        assert_eq!(world.borrow().kills, 0);
    }

    #[test]
    fn after_a_trip_the_restarted_engine_starts_clean() {
        let c = Pressure::Critical;
        let mut samples = vec![sample(c, 0); 4];
        samples.extend(vec![sample(c, 0); 3]);
        let (world, mut host, mut engine, mut dog) = rig(samples);
        let trips = run(&mut host, &mut dog, &mut engine);
        // Tripped at second 3; the count restarts at 4, so seconds 4..6 are only 2 s of critical.
        assert_eq!(trips.len(), 1);
        assert_eq!(world.borrow().kills, 1);
    }
}
