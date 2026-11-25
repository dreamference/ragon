//! Host safety for a macOS node (specs/DREAMFERENCE_MAC_NODE.md §4.3, §9's "host safety").
//!
//! The macOS twin of the GB10's two layers (`docs/dev/host-safety.md`): the pre-flight check,
//! twin of `check_host_safety()`, refuses an unsafe load before it starts, reporting every
//! refusal together; the watchdog, twin of the PSI watchdog, listens to the kernel's
//! memory-pressure events (`DISPATCH_SOURCE_TYPE_MEMORYPRESSURE`) and samples swap and the
//! compressor once a second, and on a trip sends `SIGKILL` to the engine's process group and to
//! nothing else. The sidecars the supervisor caps are reserved in the plan, twin of
//! `side_service_allowance()`.
//!
//! The decisions are plain Rust over a [`HostReadings`] value and run (and are tested) on every
//! platform; only the readers and the event source are macOS. Ragon only ever *reads*
//! `iogpu.wired_limit_mb`: raising it is the owner's choice, never made unasked.
//!
//! The thresholds marked **[E]** in the spec are provisional here ([`Thresholds::default`]); G15's
//! measurements on the gate box set them.

use std::fmt;
use std::time::Duration;
use std::time::Instant;

/// One mebibyte.
pub const MIB: u64 = 1 << 20;
/// One gibibyte.
pub const GIB: u64 = 1 << 30;

/// The kernel's memory-pressure level, as `kern.memorystatus_vm_pressure_level` and the
/// memory-pressure dispatch source report it (1 normal, 2 warning, 4 critical).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PressureLevel {
    /// No pressure.
    Normal,
    /// The system is compressing and swapping to keep up.
    Warning,
    /// Jetsam is about to kill, or is killing, processes.
    Critical,
}

impl PressureLevel {
    /// The level from the kernel's raw value or the dispatch source's flags; the highest bit set wins.
    pub fn from_raw(raw: u64) -> Self {
        if raw & 4 != 0 {
            Self::Critical
        } else if raw & 2 != 0 {
            Self::Warning
        } else {
            Self::Normal
        }
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn raw(self) -> u8 {
        match self {
            Self::Normal => 1,
            Self::Warning => 2,
            Self::Critical => 4,
        }
    }
}

/// `NSProcessInfo.thermalState` (0 nominal, 1 fair, 2 serious, 3 critical).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ThermalState {
    /// Within normal limits.
    Nominal,
    /// Slightly elevated.
    Fair,
    /// High; the system throttles.
    Serious,
    /// Critical; the system is about to shut work down.
    Critical,
}

impl ThermalState {
    /// The state from `NSProcessInfoThermalState`'s raw value; an unknown value counts as critical.
    pub fn from_raw(raw: i64) -> Self {
        match raw {
            0 => Self::Nominal,
            1 => Self::Fair,
            2 => Self::Serious,
            _ => Self::Critical,
        }
    }
}

/// What the pre-flight check reads from the host, faked in tests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostReadings {
    /// `hw.memsize`.
    pub physical_bytes: u64,
    /// Metal's `recommendedMaxWorkingSetSize`; None when there is no Metal device.
    pub recommended_max_working_set: Option<u64>,
    /// `iogpu.wired_limit_mb`; 0 is macOS's own default.
    pub wired_limit_mb: u64,
    /// `kern.memorystatus_vm_pressure_level`.
    pub pressure: PressureLevel,
    /// `vm.swapusage`'s used bytes.
    pub swap_used_bytes: u64,
    /// The compressor's occupancy, `host_statistics64`'s compressor pages times the page size.
    pub compressor_bytes: u64,
    /// `NSProcessInfo.thermalState`.
    pub thermal: ThermalState,
    /// The GPU's current allocation across every process (IOAccelerator's "In use system
    /// memory"); None when it could not be read.
    pub gpu_in_use_bytes: Option<u64>,
}

/// A sidecar the supervisor runs beside the engine, under its memory cap (§4.5, §4.7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sidecar {
    /// The name the refusal names, e.g. "the forge, ragon-forge".
    pub name: String,
    /// The supervisor's memory cap for it.
    pub cap_bytes: u64,
    /// Whether it is switched on.
    pub on: bool,
}

/// The sidecars the pre-load check sets aside: each one switched on with a cap, as (name, bytes).
///
/// The twin of `side_service_allowance()`: a sidecar's cap is reserved in the plan whether or not
/// it has grown to it yet, so the engine never plans into memory a capped sidecar may take.
pub fn side_service_allowance(sidecars: &[Sidecar]) -> Vec<(String, u64)> {
    sidecars
        .iter()
        .filter(|s| s.on && s.cap_bytes > 0)
        .map(|s| (s.name.clone(), s.cap_bytes))
        .collect()
}

/// The load the supervisor is about to start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadPlan {
    /// `ragon-serve`'s own plan: false when it exited 3 (Metal spec §4.8).
    pub engine_plan_fits: bool,
    /// The GPU working set the recipe needs.
    pub recipe_bytes: u64,
    /// What the recipe leaves to macOS and the user's applications.
    pub reserve_bytes: u64,
    /// [`side_service_allowance`]'s list.
    pub sidecars: Vec<(String, u64)>,
}

impl LoadPlan {
    /// The bytes reserved for the sidecars.
    pub fn sidecar_bytes(&self) -> u64 {
        self.sidecars.iter().map(|(_, b)| *b).sum()
    }
}

/// `ragon-serve`'s plan from its exit code: 0 fits, 3 does not; anything else is not a plan.
pub fn engine_plan_from_exit(code: Option<i32>) -> Result<bool, String> {
    match code {
        Some(0) => Ok(true),
        Some(3) => Ok(false),
        Some(other) => Err(format!("ragon-serve's plan exited {other}, not 0 or 3")),
        None => Err("ragon-serve's plan was killed by a signal".to_string()),
    }
}

/// The trip and refusal thresholds. Those marked [E] in the spec are provisional until G15.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Thresholds {
    /// Refuse a load when swap in use is above this before it starts [E].
    pub swap_used_max_bytes: u64,
    /// Refuse a load when the compressor holds more than this before it starts; None (the
    /// default) only observes it, because §4.3 refuses on swap alone until G15 says otherwise [E].
    pub compressor_max_bytes: Option<u64>,
    /// Trip when pressure stays critical this long [E].
    pub critical_for: Duration,
    /// Trip when swap grows by more than this during a load [E].
    pub swap_growth_max_bytes: u64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            swap_used_max_bytes: GIB,
            compressor_max_bytes: None,
            critical_for: Duration::from_secs(3),
            swap_growth_max_bytes: 2 * GIB,
        }
    }
}

/// One reason the pre-flight check refuses a load.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// `ragon-serve`'s own plan said no.
    EnginePlan,
    /// There is no Metal device to read a working-set cap from.
    NoMetalDevice,
    /// The recipe needs more than the GPU working-set cap minus the reserve.
    WorkingSetCap {
        /// The recipe's working set.
        need: u64,
        /// `recommendedMaxWorkingSetSize`.
        cap: u64,
        /// The recipe's reserve.
        reserve: u64,
    },
    /// `iogpu.wired_limit_mb` was raised so far that macOS and the sidecars keep less than their reserve.
    WiredLimitRaised {
        /// The sysctl's value.
        limit_mb: u64,
        /// What the limit leaves outside the GPU.
        left: u64,
        /// The reserve plus the sidecars.
        needed: u64,
    },
    /// Memory pressure is at warning or critical now.
    Pressure(PressureLevel),
    /// Swap in use is above the threshold.
    SwapInUse {
        /// Swap in use.
        used: u64,
        /// The threshold.
        max: u64,
    },
    /// The compressor holds more than the threshold.
    Compressor {
        /// The compressor's occupancy.
        bytes: u64,
        /// The threshold.
        max: u64,
    },
    /// The thermal state is critical.
    Thermal,
    /// The plan itself (the recipe, the reserve and the sidecars) is bigger than the Mac's memory.
    PlanTooBig {
        /// The plan.
        plan: u64,
        /// Physical memory.
        physical: u64,
    },
    /// Another GPU user's allocation leaves too little for the plan.
    GpuBusy {
        /// The GPU's current allocation.
        in_use: u64,
        /// The plan: the recipe, the reserve and the sidecars.
        plan: u64,
        /// Physical memory.
        physical: u64,
    },
}

fn gib(bytes: u64) -> String {
    format!("{:.1} GiB", bytes as f64 / GIB as f64)
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EnginePlan => {
                write!(
                    f,
                    "ragon-serve's plan does not fit this Mac (its plan exited 3)"
                )
            }
            Self::NoMetalDevice => {
                write!(f, "no Metal device: the GPU working-set cap cannot be read")
            }
            Self::WorkingSetCap { need, cap, reserve } => write!(
                f,
                "the recipe needs {} of GPU memory, more than the working-set cap {} minus the reserve {}",
                gib(*need),
                gib(*cap),
                gib(*reserve)
            ),
            Self::WiredLimitRaised {
                limit_mb,
                left,
                needed,
            } => write!(
                f,
                "iogpu.wired_limit_mb is {limit_mb}, which leaves macOS {} where the reserve and the \
                 sidecars need {}; Ragon never changes it: `sudo sysctl iogpu.wired_limit_mb=0` \
                 restores macOS's default",
                gib(*left),
                gib(*needed)
            ),
            Self::Pressure(level) => {
                write!(
                    f,
                    "memory pressure is {level:?} now; close applications and try again"
                )
            }
            Self::SwapInUse { used, max } => {
                write!(
                    f,
                    "{} of swap is in use, above {} before a load",
                    gib(*used),
                    gib(*max)
                )
            }
            Self::Compressor { bytes, max } => write!(
                f,
                "the memory compressor holds {}, above {} before a load",
                gib(*bytes),
                gib(*max)
            ),
            Self::Thermal => write!(f, "the thermal state is critical; let the Mac cool down"),
            Self::PlanTooBig { plan, physical } => write!(
                f,
                "the plan (the recipe, the reserve and the sidecars) needs {}, more than the Mac's {}",
                gib(*plan),
                gib(*physical)
            ),
            Self::GpuBusy {
                in_use,
                plan,
                physical,
            } => write!(
                f,
                "the GPU already holds {}, which leaves less than the plan's {} of {}",
                gib(*in_use),
                gib(*plan),
                gib(*physical)
            ),
        }
    }
}

/// The pre-flight check over the readings: Ok, or every refusal together (as `check_host_safety()`
/// reports them, so a user fixes them in one round).
pub fn check_host_safety(
    plan: &LoadPlan,
    host: &HostReadings,
    th: &Thresholds,
) -> Result<(), Vec<Refusal>> {
    let mut refusals = Vec::new();
    let sidecars = plan.sidecar_bytes();
    if !plan.engine_plan_fits {
        refusals.push(Refusal::EnginePlan);
    }
    match host.recommended_max_working_set {
        None => refusals.push(Refusal::NoMetalDevice),
        Some(cap) if plan.recipe_bytes > cap.saturating_sub(plan.reserve_bytes) => {
            refusals.push(Refusal::WorkingSetCap {
                need: plan.recipe_bytes,
                cap,
                reserve: plan.reserve_bytes,
            });
        }
        Some(_) => {}
    }
    if host.wired_limit_mb > 0 {
        let left = host
            .physical_bytes
            .saturating_sub(host.wired_limit_mb.saturating_mul(MIB));
        let needed = plan.reserve_bytes.saturating_add(sidecars);
        if left < needed {
            refusals.push(Refusal::WiredLimitRaised {
                limit_mb: host.wired_limit_mb,
                left,
                needed,
            });
        }
    }
    if host.pressure >= PressureLevel::Warning {
        refusals.push(Refusal::Pressure(host.pressure));
    }
    if host.swap_used_bytes > th.swap_used_max_bytes {
        refusals.push(Refusal::SwapInUse {
            used: host.swap_used_bytes,
            max: th.swap_used_max_bytes,
        });
    }
    if let Some(max) = th.compressor_max_bytes
        && host.compressor_bytes > max
    {
        refusals.push(Refusal::Compressor {
            bytes: host.compressor_bytes,
            max,
        });
    }
    if host.thermal == ThermalState::Critical {
        refusals.push(Refusal::Thermal);
    }
    // The sidecars are reserved here whatever else could be read: an unreadable GPU allocation
    // counts as none, never as a skipped check.
    let plan_bytes = plan
        .recipe_bytes
        .saturating_add(plan.reserve_bytes)
        .saturating_add(sidecars);
    let in_use = host.gpu_in_use_bytes.unwrap_or(0);
    if plan_bytes > host.physical_bytes {
        refusals.push(Refusal::PlanTooBig {
            plan: plan_bytes,
            physical: host.physical_bytes,
        });
    } else if host.physical_bytes.saturating_sub(in_use) < plan_bytes {
        refusals.push(Refusal::GpuBusy {
            in_use,
            plan: plan_bytes,
            physical: host.physical_bytes,
        });
    }
    if refusals.is_empty() {
        Ok(())
    } else {
        Err(refusals)
    }
}

/// IOAccelerator's "In use system memory" from `ioreg -r -d 1 -c IOAccelerator`, summed over
/// every accelerator listed; None when no line carries it.
pub fn parse_gpu_in_use(ioreg: &str) -> Option<u64> {
    const KEY: &str = "\"In use system memory\"=";
    let mut total: Option<u64> = None;
    for line in ioreg.lines() {
        let mut rest = line;
        while let Some(at) = rest.find(KEY) {
            rest = &rest[at + KEY.len()..];
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            if let Ok(v) = digits.parse::<u64>() {
                total = Some(total.unwrap_or(0).saturating_add(v));
            }
        }
    }
    total
}

/// What the watchdog guards: a load, during which swap growth trips too, or serving.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// The engine is loading; swap growth since the load began trips.
    Loading,
    /// The engine is serving; only sustained critical pressure trips.
    Serving,
}

/// One sample the watchdog takes each second.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// The pressure level: the latest dispatch event's, or the sysctl's if higher.
    pub pressure: PressureLevel,
    /// Swap in use.
    pub swap_used_bytes: u64,
    /// The compressor's occupancy.
    pub compressor_bytes: u64,
}

/// Why the watchdog tripped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TripReason {
    /// Pressure stayed critical for at least the threshold.
    PressureCritical {
        /// How long it had been critical.
        for_: Duration,
    },
    /// Swap grew by more than the threshold during the load.
    SwapGrowth {
        /// The growth since the load began.
        grew: u64,
        /// The threshold.
        max: u64,
    },
}

/// A trip: the reason and the sample that fired it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trip {
    /// Why.
    pub reason: TripReason,
    /// The sample at the trip.
    pub sample: Sample,
}

impl fmt::Display for Trip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.reason {
            TripReason::PressureCritical { for_ } => write!(
                f,
                "memory pressure critical for {:.1} s",
                for_.as_secs_f64()
            )?,
            TripReason::SwapGrowth { grew, max } => write!(
                f,
                "swap grew by {} during the load, above {}",
                gib(grew),
                gib(max)
            )?,
        }
        write!(
            f,
            " (swap {}, compressor {}); the engine was killed",
            gib(self.sample.swap_used_bytes),
            gib(self.sample.compressor_bytes)
        )
    }
}

/// The watchdog's state machine, fed by the dispatch source's events and the 1 s sampler. It
/// allocates nothing, and trips at most once.
#[derive(Clone, Debug)]
pub struct Watchdog {
    th: Thresholds,
    phase: Phase,
    load_swap_baseline: u64,
    critical_since: Option<Instant>,
    tripped: bool,
}

impl Watchdog {
    /// A watchdog for a load that starts now with `swap_used_bytes` of swap in use.
    pub fn start_load(th: Thresholds, swap_used_bytes: u64) -> Self {
        Self {
            th,
            phase: Phase::Loading,
            load_swap_baseline: swap_used_bytes,
            critical_since: None,
            tripped: false,
        }
    }

    /// The load is done: from now on only sustained critical pressure trips.
    pub fn loaded(&mut self) {
        self.phase = Phase::Serving;
    }

    /// The phase.
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Whether it has tripped.
    pub fn tripped(&self) -> bool {
        self.tripped
    }

    /// A memory-pressure event from the dispatch source.
    pub fn on_pressure(&mut self, level: PressureLevel, now: Instant) -> Option<Trip> {
        self.note_pressure(level, now);
        let sample = Sample {
            pressure: level,
            swap_used_bytes: self.load_swap_baseline,
            compressor_bytes: 0,
        };
        self.check_critical(sample, now)
    }

    /// The 1 s sample of pressure, swap and the compressor.
    pub fn on_sample(&mut self, sample: Sample, now: Instant) -> Option<Trip> {
        self.note_pressure(sample.pressure, now);
        if let Some(trip) = self.check_critical(sample, now) {
            return Some(trip);
        }
        if self.tripped || self.phase != Phase::Loading {
            return None;
        }
        let grew = sample
            .swap_used_bytes
            .saturating_sub(self.load_swap_baseline);
        if grew > self.th.swap_growth_max_bytes {
            self.tripped = true;
            return Some(Trip {
                reason: TripReason::SwapGrowth {
                    grew,
                    max: self.th.swap_growth_max_bytes,
                },
                sample,
            });
        }
        None
    }

    fn note_pressure(&mut self, level: PressureLevel, now: Instant) {
        if level == PressureLevel::Critical {
            self.critical_since.get_or_insert(now);
        } else {
            self.critical_since = None;
        }
    }

    fn check_critical(&mut self, sample: Sample, now: Instant) -> Option<Trip> {
        if self.tripped {
            return None;
        }
        let since = self.critical_since?;
        let for_ = now.saturating_duration_since(since);
        if for_ >= self.th.critical_for {
            self.tripped = true;
            return Some(Trip {
                reason: TripReason::PressureCritical { for_ },
                sample,
            });
        }
        None
    }
}

/// Sends `SIGKILL` to a process group; faked in tests.
pub trait GroupKiller {
    /// Kill process group `pgid`.
    fn kill_group(&self, pgid: i32) -> std::io::Result<()>;
}

/// The real killer: one `killpg(2)`, no daemon or container in the path.
#[cfg(unix)]
pub struct Killpg;

#[cfg(unix)]
impl GroupKiller for Killpg {
    fn kill_group(&self, pgid: i32) -> std::io::Result<()> {
        // SAFETY: killpg takes plain integers and touches no memory of ours.
        if unsafe { libc::killpg(pgid, libc::SIGKILL) } == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
}

/// Kills the engine's process group and nothing else: refuses a group that is not a real one
/// (0 or 1, which would mean "my own group" or init) or the supervisor's own group.
pub fn kill_engine(
    engine_pgid: i32,
    own_pgid: i32,
    killer: &dyn GroupKiller,
) -> std::io::Result<()> {
    if engine_pgid <= 1 || engine_pgid == own_pgid {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("refusing to kill process group {engine_pgid}: not the engine's own group"),
        ));
    }
    killer.kill_group(engine_pgid)
}

/// One watchdog step: feeds the sample and, on a trip, kills the engine's group (and nothing
/// else) before anything is formatted or logged. Returns the trip and the kill's outcome.
pub fn watchdog_step(
    dog: &mut Watchdog,
    sample: Sample,
    now: Instant,
    engine_pgid: i32,
    own_pgid: i32,
    killer: &dyn GroupKiller,
) -> Option<(Trip, std::io::Result<()>)> {
    let trip = dog.on_sample(sample, now)?;
    Some((trip, kill_engine(engine_pgid, own_pgid, killer)))
}

#[cfg(target_os = "macos")]
pub use macos::{WatchdogHandle, read_host, spawn_watchdog};

#[cfg(target_os = "macos")]
mod macos {
    //! The readers and the event source, macOS only.

    use super::*;
    use std::ffi::CStr;
    use std::ffi::c_char;
    use std::ffi::c_void;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::AtomicU8;
    use std::sync::atomic::Ordering;

    /// Reads a sysctl by name. The only sysctl call in this module, and it never writes: the new
    /// value is null.
    fn sysctl_read<T: Copy + Default>(name: &CStr) -> std::io::Result<T> {
        let mut value = T::default();
        let mut len = std::mem::size_of::<T>();
        // SAFETY: `value` is a T of `len` bytes; a null new value with length 0 is a read.
        let rc = unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                (&raw mut value).cast::<c_void>(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(value)
    }

    /// `struct xsw_usage` from <sys/sysctl.h>.
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct XswUsage {
        total: u64,
        avail: u64,
        used: u64,
        pagesize: u32,
        encrypted: u32,
    }

    fn swap_used() -> std::io::Result<u64> {
        Ok(sysctl_read::<XswUsage>(c"vm.swapusage")?.used)
    }

    fn pressure_now() -> PressureLevel {
        sysctl_read::<u32>(c"kern.memorystatus_vm_pressure_level")
            .map(|v| PressureLevel::from_raw(u64::from(v)))
            .unwrap_or(PressureLevel::Normal)
    }

    #[allow(deprecated)]
    fn compressor_bytes() -> std::io::Result<u64> {
        let mut stats: libc::vm_statistics64 = unsafe { std::mem::zeroed() };
        let mut count = libc::HOST_VM_INFO64_COUNT;
        // SAFETY: `stats` is a vm_statistics64 and `count` its size in integers.
        let rc = unsafe {
            libc::host_statistics64(
                libc::mach_host_self(),
                libc::HOST_VM_INFO64,
                (&raw mut stats).cast(),
                &mut count,
            )
        };
        if rc != 0 {
            return Err(std::io::Error::other(format!(
                "host_statistics64 returned {rc}"
            )));
        }
        let page = sysctl_read::<u64>(c"hw.pagesize").unwrap_or(16384);
        Ok(u64::from(stats.compressor_page_count).saturating_mul(page))
    }

    unsafe extern "C" {
        fn dlopen(path: *const c_char, mode: i32) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }
    const RTLD_LAZY: i32 = 1;

    /// Opens a system library and looks a symbol up; the library stays loaded (never dlclosed).
    fn symbol(lib: &CStr, name: &CStr) -> Option<*mut c_void> {
        // SAFETY: both are NUL-terminated paths and names.
        let handle = unsafe { dlopen(lib.as_ptr(), RTLD_LAZY) };
        if handle.is_null() {
            return None;
        }
        // SAFETY: as above.
        let sym = unsafe { dlsym(handle, name.as_ptr()) };
        (!sym.is_null()).then_some(sym)
    }

    const OBJC: &CStr = c"/usr/lib/libobjc.A.dylib";

    struct Objc {
        sel: unsafe extern "C" fn(*const c_char) -> *mut c_void,
        class: unsafe extern "C" fn(*const c_char) -> *mut c_void,
        send: *mut c_void,
        release: unsafe extern "C" fn(*mut c_void),
        pool_push: unsafe extern "C" fn() -> *mut c_void,
        pool_pop: unsafe extern "C" fn(*mut c_void),
    }

    fn objc() -> Option<Objc> {
        // SAFETY: each symbol has the C signature it is cast to (objc/runtime.h, objc/objc-internal.h).
        unsafe {
            Some(Objc {
                sel: std::mem::transmute::<*mut c_void, _>(symbol(OBJC, c"sel_registerName")?),
                class: std::mem::transmute::<*mut c_void, _>(symbol(OBJC, c"objc_getClass")?),
                send: symbol(OBJC, c"objc_msgSend")?,
                release: std::mem::transmute::<*mut c_void, _>(symbol(OBJC, c"objc_release")?),
                pool_push: std::mem::transmute::<*mut c_void, _>(symbol(
                    OBJC,
                    c"objc_autoreleasePoolPush",
                )?),
                pool_pop: std::mem::transmute::<*mut c_void, _>(symbol(
                    OBJC,
                    c"objc_autoreleasePoolPop",
                )?),
            })
        }
    }

    /// Metal's `recommendedMaxWorkingSetSize`, through the system default device. Metal is loaded
    /// only here, so a `ragon` that never checks never loads it.
    fn recommended_max_working_set() -> Option<u64> {
        let rt = objc()?;
        let create = symbol(
            c"/System/Library/Frameworks/Metal.framework/Metal",
            c"MTLCreateSystemDefaultDevice",
        )?;
        // SAFETY: MTLCreateSystemDefaultDevice returns a retained id<MTLDevice> or nil;
        // objc_msgSend is cast to the selector's signature, NSUInteger (u64) on arm64.
        unsafe {
            let create: unsafe extern "C" fn() -> *mut c_void = std::mem::transmute(create);
            let send: unsafe extern "C" fn(*mut c_void, *mut c_void) -> u64 =
                std::mem::transmute(rt.send);
            let pool = (rt.pool_push)();
            let device = create();
            let out = if device.is_null() {
                None
            } else {
                let size = send(device, (rt.sel)(c"recommendedMaxWorkingSetSize".as_ptr()));
                (rt.release)(device);
                Some(size)
            };
            (rt.pool_pop)(pool);
            out
        }
    }

    /// `NSProcessInfo.processInfo.thermalState`; critical when Foundation cannot be reached, so a
    /// failed read refuses rather than passes.
    fn thermal_state() -> ThermalState {
        let read = || -> Option<i64> {
            let rt = objc()?;
            symbol(
                c"/System/Library/Frameworks/Foundation.framework/Foundation",
                c"NSStringFromClass",
            )?;
            // SAFETY: class methods and getters cast to their signatures (id and NSInteger).
            unsafe {
                let send_id: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
                    std::mem::transmute(rt.send);
                let send_int: unsafe extern "C" fn(*mut c_void, *mut c_void) -> i64 =
                    std::mem::transmute(rt.send);
                let pool = (rt.pool_push)();
                let class = (rt.class)(c"NSProcessInfo".as_ptr());
                let out = if class.is_null() {
                    None
                } else {
                    let info = send_id(class, (rt.sel)(c"processInfo".as_ptr()));
                    (!info.is_null()).then(|| send_int(info, (rt.sel)(c"thermalState".as_ptr())))
                };
                (rt.pool_pop)(pool);
                out
            }
        };
        read().map_or(ThermalState::Critical, ThermalState::from_raw)
    }

    fn gpu_in_use() -> Option<u64> {
        let out = std::process::Command::new("/usr/sbin/ioreg")
            .args(["-r", "-d", "1", "-c", "IOAccelerator"])
            .output()
            .ok()?;
        parse_gpu_in_use(&String::from_utf8_lossy(&out.stdout))
    }

    /// Reads the host for the pre-flight check. A reading that fails where a refusal depends on it
    /// fails closed: no Metal device refuses, an unreadable thermal state counts as critical.
    pub fn read_host() -> std::io::Result<HostReadings> {
        Ok(HostReadings {
            physical_bytes: sysctl_read::<u64>(c"hw.memsize")?,
            recommended_max_working_set: recommended_max_working_set(),
            wired_limit_mb: u64::from(sysctl_read::<u32>(c"iogpu.wired_limit_mb").unwrap_or(0)),
            pressure: PressureLevel::from_raw(u64::from(sysctl_read::<u32>(
                c"kern.memorystatus_vm_pressure_level",
            )?)),
            swap_used_bytes: swap_used()?,
            compressor_bytes: compressor_bytes()?,
            thermal: thermal_state(),
            gpu_in_use_bytes: gpu_in_use(),
        })
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
        fn dispatch_source_set_event_handler_f(
            source: *mut c_void,
            handler: extern "C" fn(*mut c_void),
        );
        fn dispatch_source_get_data(source: *mut c_void) -> usize;
        fn dispatch_resume(object: *mut c_void);
        fn dispatch_source_cancel(source: *mut c_void);
    }

    const MEMORYPRESSURE_ALL: usize = 0x01 | 0x02 | 0x04;
    const QOS_CLASS_USER_INTERACTIVE: isize = 0x21;

    /// The event handler's context: the source and the latest level. Leaked on purpose, one per
    /// engine start, because a cancelled source may still be running its last handler.
    struct EventContext {
        source: std::sync::atomic::AtomicPtr<c_void>,
        level: AtomicU8,
    }

    extern "C" fn on_pressure_event(context: *mut c_void) {
        // SAFETY: the context is the leaked EventContext set below, never freed.
        let ctx = unsafe { &*(context as *const EventContext) };
        let source = ctx.source.load(Ordering::Acquire);
        if source.is_null() {
            return;
        }
        // SAFETY: the source is live until cancelled, and a cancelled source runs no new handler.
        let data = unsafe { dispatch_source_get_data(source) };
        ctx.level.store(
            PressureLevel::from_raw(data as u64).raw(),
            Ordering::Release,
        );
    }

    /// The running watchdog; dropping it stops it without killing anything.
    pub struct WatchdogHandle {
        stop: Arc<AtomicBool>,
        loaded: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<Option<Trip>>>,
        ctx: &'static EventContext,
    }

    impl WatchdogHandle {
        /// The load is done: only sustained critical pressure trips from now on.
        pub fn loaded(&self) {
            self.loaded.store(true, Ordering::Release);
        }

        /// Stops the watchdog and returns the trip, if it tripped.
        pub fn stop(mut self) -> Option<Trip> {
            self.halt()
        }

        fn halt(&mut self) -> Option<Trip> {
            self.stop.store(true, Ordering::Release);
            let source = self.ctx.source.swap(std::ptr::null_mut(), Ordering::AcqRel);
            if !source.is_null() {
                // SAFETY: the source was created and resumed by spawn_watchdog.
                unsafe { dispatch_source_cancel(source) };
            }
            self.thread.take().and_then(|t| t.join().ok().flatten())
        }
    }

    impl Drop for WatchdogHandle {
        fn drop(&mut self) {
            self.halt();
        }
    }

    /// Starts the watchdog for an engine loading in process group `engine_pgid`: the dispatch
    /// source for pressure events, and a thread sampling once a second that kills the group on a
    /// trip. The thread allocates nothing until it trips.
    pub fn spawn_watchdog(engine_pgid: i32, th: Thresholds) -> std::io::Result<WatchdogHandle> {
        // SAFETY: getpgrp has no preconditions.
        let own_pgid = unsafe { libc::getpgrp() };
        if engine_pgid <= 1 || engine_pgid == own_pgid {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "the engine must run in its own process group",
            ));
        }
        let ctx: &'static EventContext = Box::leak(Box::new(EventContext {
            source: std::sync::atomic::AtomicPtr::new(std::ptr::null_mut()),
            level: AtomicU8::new(PressureLevel::Normal.raw()),
        }));
        // SAFETY: the type symbol is libdispatch's; the queue is a global one.
        let source = unsafe {
            dispatch_source_create(
                &raw const _dispatch_source_type_memorypressure,
                0,
                MEMORYPRESSURE_ALL,
                dispatch_get_global_queue(QOS_CLASS_USER_INTERACTIVE, 0),
            )
        };
        if source.is_null() {
            return Err(std::io::Error::other(
                "dispatch_source_create returned null",
            ));
        }
        ctx.source.store(source, Ordering::Release);
        // SAFETY: the context outlives the source (leaked); the handler matches the signature.
        unsafe {
            dispatch_set_context(source, (ctx as *const EventContext).cast_mut().cast());
            dispatch_source_set_event_handler_f(source, on_pressure_event);
            dispatch_resume(source);
        }
        let stop = Arc::new(AtomicBool::new(false));
        let loaded = Arc::new(AtomicBool::new(false));
        let baseline = swap_used()?;
        let (t_stop, t_loaded) = (Arc::clone(&stop), Arc::clone(&loaded));
        let thread = std::thread::Builder::new()
            .name("ragon-host-watchdog".to_string())
            .spawn(move || {
                let mut dog = Watchdog::start_load(th, baseline);
                while !t_stop.load(Ordering::Acquire) {
                    if t_loaded.load(Ordering::Acquire) && dog.phase() == Phase::Loading {
                        dog.loaded();
                    }
                    let event =
                        PressureLevel::from_raw(u64::from(ctx.level.load(Ordering::Acquire)));
                    let sample = Sample {
                        pressure: event.max(pressure_now()),
                        swap_used_bytes: swap_used().unwrap_or(baseline),
                        compressor_bytes: compressor_bytes().unwrap_or(0),
                    };
                    if let Some((trip, killed)) = watchdog_step(
                        &mut dog,
                        sample,
                        Instant::now(),
                        engine_pgid,
                        own_pgid,
                        &Killpg,
                    ) {
                        eprintln!("ragon: host safety: {trip}");
                        if let Err(e) = killed {
                            eprintln!("ragon: host safety: the kill failed: {e}");
                        }
                        return Some(trip);
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }
                None
            })?;
        Ok(WatchdogHandle {
            stop,
            loaded,
            thread: Some(thread),
            ctx,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn calm_host() -> HostReadings {
        HostReadings {
            physical_bytes: 128 * GIB,
            recommended_max_working_set: Some(96 * GIB),
            wired_limit_mb: 0,
            pressure: PressureLevel::Normal,
            swap_used_bytes: 0,
            compressor_bytes: GIB,
            thermal: ThermalState::Nominal,
            gpu_in_use_bytes: Some(GIB / 2),
        }
    }

    fn plan() -> LoadPlan {
        LoadPlan {
            engine_plan_fits: true,
            recipe_bytes: 60 * GIB,
            reserve_bytes: 16 * GIB,
            sidecars: side_service_allowance(&[Sidecar {
                name: "the forge, ragon-forge".to_string(),
                cap_bytes: 2 * GIB,
                on: true,
            }]),
        }
    }

    fn refusals(plan: &LoadPlan, host: &HostReadings) -> Vec<Refusal> {
        check_host_safety(plan, host, &Thresholds::default())
            .err()
            .unwrap_or_default()
    }

    #[test]
    fn a_calm_host_with_a_fitting_plan_passes() {
        assert_eq!(
            check_host_safety(&plan(), &calm_host(), &Thresholds::default()),
            Ok(())
        );
    }

    #[test]
    fn the_engines_own_plan_refuses() {
        let p = LoadPlan {
            engine_plan_fits: false,
            ..plan()
        };
        assert_eq!(refusals(&p, &calm_host()), vec![Refusal::EnginePlan]);
        assert_eq!(engine_plan_from_exit(Some(3)), Ok(false));
        assert_eq!(engine_plan_from_exit(Some(0)), Ok(true));
        assert!(engine_plan_from_exit(Some(1)).is_err());
        assert!(engine_plan_from_exit(None).is_err());
    }

    #[test]
    fn recommended_max_working_set_size_refuses_a_recipe_above_the_cap_minus_the_reserve() {
        let host = HostReadings {
            recommended_max_working_set: Some(70 * GIB),
            ..calm_host()
        };
        // 60 GiB needed, 70 - 16 = 54 GiB available.
        assert_eq!(
            refusals(&plan(), &host),
            vec![Refusal::WorkingSetCap {
                need: 60 * GIB,
                cap: 70 * GIB,
                reserve: 16 * GIB
            }]
        );
        let exact = HostReadings {
            recommended_max_working_set: Some(76 * GIB),
            ..calm_host()
        };
        assert_eq!(refusals(&plan(), &exact), vec![]);
        let none = HostReadings {
            recommended_max_working_set: None,
            ..calm_host()
        };
        assert_eq!(refusals(&plan(), &none), vec![Refusal::NoMetalDevice]);
    }

    #[test]
    fn iogpu_wired_limit_mb_raised_past_the_reserve_refuses_and_never_advises_raising_it() {
        // 120 GiB wired leaves 8 GiB where the reserve (16) and the forge (2) need 18.
        let host = HostReadings {
            wired_limit_mb: 120 * 1024,
            recommended_max_working_set: Some(118 * GIB),
            ..calm_host()
        };
        let got = refusals(&plan(), &host);
        assert_eq!(
            got,
            vec![Refusal::WiredLimitRaised {
                limit_mb: 120 * 1024,
                left: 8 * GIB,
                needed: 18 * GIB
            }]
        );
        let message = got[0].to_string();
        assert!(message.contains("never changes it"), "{message}");
        assert!(message.contains("iogpu.wired_limit_mb=0"), "{message}");
        // A raised limit that still leaves the reserve and the sidecars passes; 0 is the default.
        let fine = HostReadings {
            wired_limit_mb: 100 * 1024,
            ..calm_host()
        };
        assert_eq!(refusals(&plan(), &fine), vec![]);
    }

    #[test]
    fn the_wired_limit_is_only_ever_read() {
        let source = include_str!("host_safety_macos.rs");
        let code = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .unwrap_or(source);
        assert_eq!(
            code.matches("libc::sysctlbyname(").count(),
            1,
            "one sysctl call site"
        );
        assert!(
            code.contains("std::ptr::null_mut(),\n                0,\n"),
            "its new value is null"
        );
        assert!(!code.contains("Command::new(\"sysctl\")") && !code.contains("/usr/sbin/sysctl"));
    }

    #[test]
    fn kern_memorystatus_vm_pressure_level_warning_or_critical_refuses() {
        assert_eq!(PressureLevel::from_raw(1), PressureLevel::Normal);
        assert_eq!(PressureLevel::from_raw(2), PressureLevel::Warning);
        assert_eq!(PressureLevel::from_raw(4), PressureLevel::Critical);
        assert_eq!(PressureLevel::from_raw(0), PressureLevel::Normal);
        for level in [PressureLevel::Warning, PressureLevel::Critical] {
            let host = HostReadings {
                pressure: level,
                ..calm_host()
            };
            assert_eq!(refusals(&plan(), &host), vec![Refusal::Pressure(level)]);
        }
    }

    #[test]
    fn vm_swapusage_and_the_compressor_above_their_thresholds_refuse() {
        let swap = HostReadings {
            swap_used_bytes: 3 * GIB,
            ..calm_host()
        };
        assert_eq!(
            refusals(&plan(), &swap),
            vec![Refusal::SwapInUse {
                used: 3 * GIB,
                max: GIB
            }]
        );
        let compressed = HostReadings {
            compressor_bytes: 9 * GIB,
            ..calm_host()
        };
        // The compressor is observed only, unless a threshold is set (G15 decides).
        assert_eq!(refusals(&plan(), &compressed), vec![]);
        let strict = Thresholds {
            compressor_max_bytes: Some(8 * GIB),
            ..Thresholds::default()
        };
        assert_eq!(
            check_host_safety(&plan(), &compressed, &strict).err(),
            Some(vec![Refusal::Compressor {
                bytes: 9 * GIB,
                max: 8 * GIB
            }])
        );
    }

    #[test]
    fn a_critical_thermal_state_refuses_and_serious_does_not() {
        assert_eq!(ThermalState::from_raw(3), ThermalState::Critical);
        assert_eq!(ThermalState::from_raw(9), ThermalState::Critical);
        let hot = HostReadings {
            thermal: ThermalState::Critical,
            ..calm_host()
        };
        assert_eq!(refusals(&plan(), &hot), vec![Refusal::Thermal]);
        let warm = HostReadings {
            thermal: ThermalState::Serious,
            ..calm_host()
        };
        assert_eq!(refusals(&plan(), &warm), vec![]);
    }

    #[test]
    fn another_gpu_user_leaving_too_little_for_the_plan_refuses() {
        // 128 - 52 = 76 GiB free; the plan is 60 + 16 + 2 = 78.
        let busy = HostReadings {
            gpu_in_use_bytes: Some(52 * GIB),
            ..calm_host()
        };
        assert_eq!(
            refusals(&plan(), &busy),
            vec![Refusal::GpuBusy {
                in_use: 52 * GIB,
                plan: 78 * GIB,
                physical: 128 * GIB
            }]
        );
    }

    #[test]
    fn an_unreadable_gpu_allocation_still_checks_the_plan_against_memory() {
        let unknown = HostReadings {
            gpu_in_use_bytes: None,
            ..calm_host()
        };
        assert_eq!(refusals(&plan(), &unknown), vec![]);
        // 60 + 16 + 60 GiB of sidecars = 136 GiB, more than the Mac's 128.
        let huge = LoadPlan {
            sidecars: vec![("a sidecar".to_string(), 60 * GIB)],
            ..plan()
        };
        assert_eq!(
            refusals(&huge, &unknown),
            vec![Refusal::PlanTooBig {
                plan: 136 * GIB,
                physical: 128 * GIB
            }]
        );
        let message = Refusal::PlanTooBig {
            plan: 136 * GIB,
            physical: 128 * GIB,
        }
        .to_string();
        assert!(message.contains("136"), "{message}");
    }

    #[test]
    fn sidecars_are_reserved_in_the_plan() {
        let sidecars = [
            Sidecar {
                name: "the forge, ragon-forge".to_string(),
                cap_bytes: 2 * GIB,
                on: true,
            },
            Sidecar {
                name: "search, SearXNG".to_string(),
                cap_bytes: GIB,
                on: false,
            },
            Sidecar {
                name: "voice".to_string(),
                cap_bytes: 0,
                on: true,
            },
        ];
        assert_eq!(
            side_service_allowance(&sidecars),
            vec![("the forge, ragon-forge".to_string(), 2 * GIB)]
        );
        assert!(side_service_allowance(&[]).is_empty());
        // 128 - 50 = 78 GiB free: the plan without the forge (76) fits, with it (78) still fits,
        // a bigger forge (3 GiB, plan 79) does not.
        let host = HostReadings {
            gpu_in_use_bytes: Some(50 * GIB),
            ..calm_host()
        };
        assert_eq!(refusals(&plan(), &host), vec![]);
        let big = LoadPlan {
            sidecars: vec![("the forge".to_string(), 3 * GIB)],
            ..plan()
        };
        assert!(matches!(
            refusals(&big, &host)[..],
            [Refusal::GpuBusy { .. }]
        ));
    }

    #[test]
    fn every_refusal_is_reported_together() {
        let host = HostReadings {
            pressure: PressureLevel::Critical,
            swap_used_bytes: 4 * GIB,
            thermal: ThermalState::Critical,
            ..calm_host()
        };
        let p = LoadPlan {
            engine_plan_fits: false,
            ..plan()
        };
        assert_eq!(refusals(&p, &host).len(), 4);
    }

    #[test]
    fn ioreg_in_use_system_memory_is_parsed_and_summed() {
        let ioreg = "+-o AGXAcceleratorG17X  <class AGXAcceleratorG17X>\n    \
             \"PerformanceStatistics\" = {\"Alloc system memory\"=431554560,\"In use system memory\"=305364992,\"Tiler Heap\"=0}\n\
             \"PerformanceStatistics\" = {\"In use system memory\"=7}\n";
        assert_eq!(parse_gpu_in_use(ioreg), Some(305_364_999));
        assert_eq!(parse_gpu_in_use("nothing here"), None);
    }

    fn sample(pressure: PressureLevel, swap: u64) -> Sample {
        Sample {
            pressure,
            swap_used_bytes: swap,
            compressor_bytes: 0,
        }
    }

    #[test]
    fn critical_pressure_for_three_seconds_trips_once() {
        let t0 = Instant::now();
        let mut dog = Watchdog::start_load(Thresholds::default(), 0);
        assert_eq!(dog.on_pressure(PressureLevel::Critical, t0), None);
        assert_eq!(
            dog.on_sample(
                sample(PressureLevel::Critical, 0),
                t0 + Duration::from_secs(2)
            ),
            None
        );
        let trip = dog.on_sample(
            sample(PressureLevel::Critical, 0),
            t0 + Duration::from_secs(3),
        );
        assert_eq!(
            trip.map(|t| t.reason),
            Some(TripReason::PressureCritical {
                for_: Duration::from_secs(3)
            })
        );
        assert!(dog.tripped());
        assert_eq!(
            dog.on_sample(
                sample(PressureLevel::Critical, 0),
                t0 + Duration::from_secs(9)
            ),
            None
        );
    }

    #[test]
    fn critical_pressure_that_eases_resets_the_clock_and_warning_never_trips() {
        let t0 = Instant::now();
        let s = Duration::from_secs;
        let mut dog = Watchdog::start_load(Thresholds::default(), 0);
        dog.on_pressure(PressureLevel::Critical, t0);
        assert_eq!(dog.on_pressure(PressureLevel::Warning, t0 + s(2)), None);
        assert_eq!(
            dog.on_sample(sample(PressureLevel::Critical, 0), t0 + s(3)),
            None
        );
        assert_eq!(
            dog.on_sample(sample(PressureLevel::Critical, 0), t0 + s(5)),
            None
        );
        assert!(
            dog.on_sample(sample(PressureLevel::Critical, 0), t0 + s(6))
                .is_some()
        );
        let mut calm = Watchdog::start_load(Thresholds::default(), 0);
        for i in 0..60 {
            assert_eq!(
                calm.on_sample(sample(PressureLevel::Warning, 0), t0 + s(i)),
                None
            );
        }
    }

    #[test]
    fn critical_pressure_trips_while_serving_too() {
        let t0 = Instant::now();
        let mut dog = Watchdog::start_load(Thresholds::default(), 0);
        dog.loaded();
        dog.on_pressure(PressureLevel::Critical, t0);
        assert!(
            dog.on_pressure(PressureLevel::Critical, t0 + Duration::from_secs(4))
                .is_some()
        );
    }

    #[test]
    fn swap_growth_during_the_load_trips_and_not_while_serving() {
        let t0 = Instant::now();
        let mut dog = Watchdog::start_load(Thresholds::default(), 5 * GIB);
        assert_eq!(
            dog.on_sample(sample(PressureLevel::Normal, 7 * GIB), t0),
            None
        );
        let trip = dog.on_sample(sample(PressureLevel::Warning, 7 * GIB + 1), t0);
        assert_eq!(
            trip.map(|t| t.reason),
            Some(TripReason::SwapGrowth {
                grew: 2 * GIB + 1,
                max: 2 * GIB
            })
        );
        let message = trip.map(|t| t.to_string()).unwrap_or_default();
        assert!(message.contains("the engine was killed"), "{message}");
        let mut serving = Watchdog::start_load(Thresholds::default(), 0);
        serving.loaded();
        assert_eq!(
            serving.on_sample(sample(PressureLevel::Normal, 40 * GIB), t0),
            None
        );
    }

    struct FakeKiller(RefCell<Vec<i32>>);
    impl GroupKiller for FakeKiller {
        fn kill_group(&self, pgid: i32) -> std::io::Result<()> {
            self.0.borrow_mut().push(pgid);
            Ok(())
        }
    }

    #[test]
    fn every_trip_kills_the_engine_once_and_no_trip_kills_nothing() {
        let t0 = Instant::now();
        let s = Duration::from_secs;
        // No trip: calm samples for a minute kill nothing.
        let killer = FakeKiller(RefCell::new(Vec::new()));
        let mut dog = Watchdog::start_load(Thresholds::default(), 0);
        for i in 0..60 {
            assert!(
                watchdog_step(
                    &mut dog,
                    sample(PressureLevel::Warning, GIB),
                    t0 + s(i),
                    4242,
                    100,
                    &killer
                )
                .is_none()
            );
        }
        assert!(killer.0.borrow().is_empty());
        // Critical pressure: one kill of the engine's group, then none however long it lasts.
        let killer = FakeKiller(RefCell::new(Vec::new()));
        let mut dog = Watchdog::start_load(Thresholds::default(), 0);
        let mut trips = Vec::new();
        for i in 0..10 {
            if let Some((trip, killed)) = watchdog_step(
                &mut dog,
                sample(PressureLevel::Critical, 0),
                t0 + s(i),
                4242,
                100,
                &killer,
            ) {
                assert!(killed.is_ok());
                trips.push(trip.reason);
            }
        }
        assert!(matches!(trips[..], [TripReason::PressureCritical { .. }]));
        assert_eq!(*killer.0.borrow(), vec![4242]);
        // Swap growth during the load: one kill of the engine's group.
        let killer = FakeKiller(RefCell::new(Vec::new()));
        let mut dog = Watchdog::start_load(Thresholds::default(), 0);
        let step = watchdog_step(
            &mut dog,
            sample(PressureLevel::Normal, 3 * GIB),
            t0,
            4242,
            100,
            &killer,
        );
        assert!(matches!(
            step.map(|(t, _)| t.reason),
            Some(TripReason::SwapGrowth { .. })
        ));
        assert!(
            watchdog_step(
                &mut dog,
                sample(PressureLevel::Normal, 9 * GIB),
                t0 + s(1),
                4242,
                100,
                &killer
            )
            .is_none()
        );
        assert_eq!(*killer.0.borrow(), vec![4242]);
        // A trip never kills the supervisor's own group: the kill is refused, nothing is signalled.
        let killer = FakeKiller(RefCell::new(Vec::new()));
        let mut dog = Watchdog::start_load(Thresholds::default(), 0);
        let step = watchdog_step(
            &mut dog,
            sample(PressureLevel::Normal, 3 * GIB),
            t0,
            100,
            100,
            &killer,
        );
        assert!(step.is_some_and(|(_, killed)| killed.is_err()));
        assert!(killer.0.borrow().is_empty());
    }

    #[test]
    fn a_trip_kills_the_engines_group_and_nothing_else() {
        let killer = FakeKiller(RefCell::new(Vec::new()));
        assert!(kill_engine(4242, 100, &killer).is_ok());
        assert!(
            kill_engine(100, 100, &killer).is_err(),
            "the supervisor's own group"
        );
        assert!(
            kill_engine(0, 100, &killer).is_err(),
            "0 means the caller's group"
        );
        assert!(kill_engine(1, 100, &killer).is_err(), "init");
        assert!(kill_engine(-5, 100, &killer).is_err());
        assert_eq!(*killer.0.borrow(), vec![4242]);
    }

    #[cfg(unix)]
    #[test]
    fn killpg_kills_the_engines_group_and_the_caller_survives() {
        use std::os::unix::process::CommandExt;
        let mut child = match std::process::Command::new("sleep")
            .arg("30")
            .process_group(0)
            .spawn()
        {
            Ok(c) => c,
            Err(e) => panic!("spawn sleep: {e}"),
        };
        let pgid = child.id() as i32;
        // SAFETY: getpgrp has no preconditions.
        let own = unsafe { libc::getpgrp() };
        assert!(kill_engine(pgid, own, &Killpg).is_ok());
        let status = child.wait().map(|s| {
            use std::os::unix::process::ExitStatusExt;
            s.signal()
        });
        assert_eq!(status.ok().flatten(), Some(libc::SIGKILL));
    }

    /// Reads the real host; run by hand on a Mac: `cargo test ... -- --ignored real_host`.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn real_host_readings() {
        let host = read_host();
        eprintln!("{host:?}");
        let host = match host {
            Ok(h) => h,
            Err(e) => panic!("read_host: {e}"),
        };
        assert!(host.physical_bytes > 0);
        assert!(host.recommended_max_working_set.is_some_and(|c| c > 0));
        let dog = match spawn_watchdog(i32::MAX, Thresholds::default()) {
            Ok(dog) => dog,
            Err(e) => panic!("spawn_watchdog: {e}"),
        };
        // Long enough for the dispatch source and at least one 1 s sample.
        std::thread::sleep(Duration::from_millis(1500));
        dog.loaded();
        assert_eq!(dog.stop(), None);
    }
}
