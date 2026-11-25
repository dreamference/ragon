//! The platform-neutral supervisor: a state machine that starts the engine, probes it, restarts it
//! with backoff and stops it. No I/O of its own: the engine and the probe come in as traits and the
//! clock as an argument to [`Supervisor::tick`], so the tests drive it with a fake clock and the
//! Windows service, the Mac launchd job and the foreground `run` share one implementation.

use std::time::{Duration, Instant};

/// Restart delays: `base`, doubled per consecutive failure, capped at `cap`. With the defaults
/// (1 s, 30 s): 1, 2, 4, 8, 16, 30, 30, ...
#[derive(Clone, Debug)]
pub struct Backoff {
    base: Duration,
    cap: Duration,
    attempt: u32,
}

impl Backoff {
    pub fn new(base: Duration, cap: Duration) -> Self {
        Self { base, cap, attempt: 0 }
    }

    /// The delay before the next start; each call counts one more consecutive failure.
    pub fn next_delay(&mut self) -> Duration {
        let factor = 1u32.checked_shl(self.attempt.min(31)).unwrap_or(u32::MAX);
        self.attempt = self.attempt.saturating_add(1);
        self.base.saturating_mul(factor).min(self.cap)
    }

    pub fn reset(&mut self) {
        self.attempt = 0;
    }

    /// Consecutive failures counted since the last reset.
    pub fn attempt(&self) -> u32 {
        self.attempt
    }
}

/// What the supervisor is doing. The names are the `state:` values of `server status`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Not started yet, or stopped on request.
    Stopped,
    /// The engine runs and has not answered the probe yet.
    Starting,
    /// The engine answers the probe.
    Healthy,
    /// The engine answered before and has missed the probe since (fewer than `unhealthy_after`).
    Unhealthy,
    /// The engine exited or was killed; the next start is due at `next_start`.
    Waiting,
    /// The engine exited with a code in `no_retry_exit_codes`: reported, not restarted.
    Refused,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Stopped => "stopped",
            State::Starting => "starting",
            State::Healthy => "healthy",
            State::Unhealthy => "unhealthy",
            State::Waiting => "waiting",
            State::Refused => "refused",
        }
    }
}

/// The engine process as the supervisor sees it.
pub trait Engine {
    /// Starts the engine and returns its pid.
    fn start(&mut self) -> std::io::Result<u32>;
    /// `Some(code)` once the engine has exited (killed by a signal: -1), `None` while it runs.
    fn try_wait(&mut self) -> std::io::Result<Option<i32>>;
    /// Kills the engine (and on Windows everything in its Job Object) and reaps it.
    fn kill(&mut self) -> std::io::Result<()>;
}

/// One health check of the engine (`GET /v1/models` answering 200 in production).
pub trait Probe {
    fn healthy(&mut self) -> bool;
}

/// The knobs. [`Policy::default`] is the node's: backoff 1 s doubling to 30 s, reset after 60 s
/// healthy, ten minutes to load a model, three missed probes two seconds apart make a restart.
#[derive(Clone, Debug)]
pub struct Policy {
    pub backoff_base: Duration,
    pub backoff_cap: Duration,
    /// Healthy this long resets the backoff to `backoff_base`.
    pub healthy_reset: Duration,
    /// A start that has not answered the probe after this long is killed and counted as a failure.
    pub start_timeout: Duration,
    pub probe_interval: Duration,
    /// Consecutive missed probes after being healthy that make a restart.
    pub unhealthy_after: u32,
    /// Exit codes that mean "do not retry" (the engine refused to run: wrong model, no GPU).
    pub no_retry_exit_codes: Vec<i32>,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            backoff_base: Duration::from_secs(1),
            backoff_cap: Duration::from_secs(30),
            healthy_reset: Duration::from_secs(60),
            start_timeout: Duration::from_secs(600),
            probe_interval: Duration::from_secs(2),
            unhealthy_after: 3,
            no_retry_exit_codes: vec![EXIT_REFUSED],
        }
    }
}

/// The exit code an engine uses to say "do not restart me".
pub const EXIT_REFUSED: i32 = 3;

/// What `server status` prints, taken from the supervisor at one instant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub state: State,
    pub engine_pid: Option<u32>,
    /// Engine restarts since the supervisor started (each start after the first).
    pub restarts: u64,
    pub last_exit: Option<i32>,
    /// Seconds until the next start while `Waiting` (rounded up).
    pub next_start_s: Option<u64>,
    pub last_error: Option<String>,
}

pub struct Supervisor<E: Engine, P: Probe> {
    engine: E,
    probe: P,
    policy: Policy,
    backoff: Backoff,
    state: State,
    pid: Option<u32>,
    starts: u64,
    last_exit: Option<i32>,
    last_error: Option<String>,
    started_at: Option<Instant>,
    healthy_since: Option<Instant>,
    next_start: Option<Instant>,
    missed_probes: u32,
    stopped: bool,
}

impl<E: Engine, P: Probe> Supervisor<E, P> {
    pub fn new(engine: E, probe: P, policy: Policy) -> Self {
        let backoff = Backoff::new(policy.backoff_base, policy.backoff_cap);
        Self {
            engine,
            probe,
            policy,
            backoff,
            state: State::Stopped,
            pid: None,
            starts: 0,
            last_exit: None,
            last_error: None,
            started_at: None,
            healthy_since: None,
            next_start: None,
            missed_probes: 0,
            stopped: false,
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn engine(&mut self) -> &mut E {
        &mut self.engine
    }

    /// Advances the state machine to `now` and returns how long the caller may sleep before the
    /// next tick. A supervisor stopped by [`Supervisor::stop`] stays stopped.
    pub fn tick(&mut self, now: Instant) -> Duration {
        match self.state {
            State::Stopped => {
                if self.stopped {
                    return self.policy.probe_interval;
                }
                self.start(now)
            }
            State::Waiting => match self.next_start {
                Some(due) if now < due => (due - now).min(self.policy.probe_interval),
                _ => self.start(now),
            },
            State::Refused => self.policy.probe_interval,
            State::Starting | State::Healthy | State::Unhealthy => self.watch(now),
        }
    }

    /// Kills the engine and stays stopped until [`Supervisor::resume`].
    pub fn stop(&mut self) {
        if matches!(self.state, State::Starting | State::Healthy | State::Unhealthy)
            && let Err(e) = self.engine.kill()
        {
            self.last_error = Some(format!("stop: {e}"));
        }
        self.state = State::Stopped;
        self.pid = None;
        self.next_start = None;
        self.stopped = true;
    }

    /// Lets a stopped (or refused) supervisor start the engine again on the next tick.
    pub fn resume(&mut self) {
        self.stopped = false;
        self.backoff.reset();
        if self.state == State::Refused {
            self.state = State::Stopped;
        }
    }

    pub fn snapshot(&self, now: Instant) -> Snapshot {
        let next_start_s = match (self.state, self.next_start) {
            (State::Waiting, Some(due)) => {
                let left = due.saturating_duration_since(now);
                Some(left.as_secs() + u64::from(left.subsec_nanos() > 0))
            }
            _ => None,
        };
        Snapshot {
            state: self.state,
            engine_pid: self.pid,
            restarts: self.starts.saturating_sub(1),
            last_exit: self.last_exit,
            next_start_s,
            last_error: self.last_error.clone(),
        }
    }

    fn start(&mut self, now: Instant) -> Duration {
        self.next_start = None;
        self.missed_probes = 0;
        self.healthy_since = None;
        self.starts += 1;
        match self.engine.start() {
            Ok(pid) => {
                self.pid = Some(pid);
                self.started_at = Some(now);
                self.state = State::Starting;
                Duration::from_millis(200).min(self.policy.probe_interval)
            }
            Err(e) => {
                self.last_error = Some(format!("start: {e}"));
                self.schedule_restart(now)
            }
        }
    }

    fn watch(&mut self, now: Instant) -> Duration {
        match self.engine.try_wait() {
            Ok(Some(code)) => return self.exited(code, now),
            Ok(None) => {}
            Err(e) => self.last_error = Some(format!("wait: {e}")),
        }
        if self.probe.healthy() {
            self.missed_probes = 0;
            let since = *self.healthy_since.get_or_insert(now);
            self.state = State::Healthy;
            if now.saturating_duration_since(since) >= self.policy.healthy_reset {
                self.backoff.reset();
            }
            return self.policy.probe_interval;
        }
        match self.state {
            State::Starting => {
                let started = self.started_at.unwrap_or(now);
                if now.saturating_duration_since(started) >= self.policy.start_timeout {
                    self.last_error = Some("start timeout: the engine never answered the probe".into());
                    return self.kill_and_restart(now);
                }
                // Poll faster while the engine loads, so a healthy start is seen quickly.
                Duration::from_millis(500).min(self.policy.probe_interval)
            }
            _ => {
                self.missed_probes += 1;
                self.healthy_since = None;
                self.state = State::Unhealthy;
                if self.missed_probes >= self.policy.unhealthy_after {
                    self.last_error = Some(format!("{} probes missed", self.missed_probes));
                    return self.kill_and_restart(now);
                }
                self.policy.probe_interval
            }
        }
    }

    fn kill_and_restart(&mut self, now: Instant) -> Duration {
        if let Err(e) = self.engine.kill() {
            self.last_error = Some(format!("kill: {e}"));
        }
        self.last_exit = None;
        self.pid = None;
        self.schedule_restart(now)
    }

    fn exited(&mut self, code: i32, now: Instant) -> Duration {
        self.last_exit = Some(code);
        self.pid = None;
        if self.policy.no_retry_exit_codes.contains(&code) {
            self.state = State::Refused;
            self.last_error = Some(format!("the engine refused to run (exit {code}); not restarted"));
            return self.policy.probe_interval;
        }
        self.schedule_restart(now)
    }

    fn schedule_restart(&mut self, now: Instant) -> Duration {
        let delay = self.backoff.next_delay();
        self.next_start = Some(now + delay);
        self.state = State::Waiting;
        delay.min(self.policy.probe_interval)
    }
}

/// `server status`'s text: one `key: value` line per field, always the same keys in the same order,
/// `none` for an absent value, so scripts and tests can read it line by line.
pub fn render_status(fields: &[(&str, String)]) -> String {
    let mut out = String::new();
    for (key, value) in fields {
        out.push_str(key);
        out.push_str(": ");
        out.push_str(if value.is_empty() { "none" } else { value });
        out.push('\n');
    }
    out
}

/// Parses [`render_status`]'s text back into pairs (unknown lines are skipped).
pub fn parse_status(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| line.split_once(": "))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect()
}

/// `2026-11-25T10:00:00Z` from seconds since the Unix epoch (UTC, no leap seconds).
pub fn utc_timestamp(unix_secs: u64) -> String {
    let days = (unix_secs / 86_400) as i64;
    let rem = unix_secs % 86_400;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Default)]
    struct World {
        running: bool,
        exit: Option<i32>,
        healthy: bool,
        starts: u32,
        kills: u32,
        fail_start: bool,
    }

    struct FakeEngine(Rc<RefCell<World>>);
    struct FakeProbe(Rc<RefCell<World>>);

    impl Engine for FakeEngine {
        fn start(&mut self) -> std::io::Result<u32> {
            let mut w = self.0.borrow_mut();
            if w.fail_start {
                return Err(std::io::Error::other("no such file"));
            }
            w.starts += 1;
            w.running = true;
            w.exit = None;
            Ok(1000 + w.starts)
        }
        fn try_wait(&mut self) -> std::io::Result<Option<i32>> {
            let mut w = self.0.borrow_mut();
            if let Some(code) = w.exit {
                w.running = false;
                return Ok(Some(code));
            }
            Ok(None)
        }
        fn kill(&mut self) -> std::io::Result<()> {
            let mut w = self.0.borrow_mut();
            w.kills += 1;
            w.running = false;
            Ok(())
        }
    }

    impl Probe for FakeProbe {
        fn healthy(&mut self) -> bool {
            let w = self.0.borrow();
            w.running && w.healthy
        }
    }

    fn setup() -> (Rc<RefCell<World>>, Supervisor<FakeEngine, FakeProbe>, Instant) {
        let world = Rc::new(RefCell::new(World { healthy: true, ..World::default() }));
        let sup = Supervisor::new(FakeEngine(world.clone()), FakeProbe(world.clone()), Policy::default());
        (world, sup, Instant::now())
    }

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    #[test]
    fn backoff_doubles_from_one_second_and_caps_at_thirty() {
        let mut b = Backoff::new(secs(1), secs(30));
        let got: Vec<u64> = (0..8).map(|_| b.next_delay().as_secs()).collect();
        assert_eq!(got, [1, 2, 4, 8, 16, 30, 30, 30]);
        b.reset();
        assert_eq!(b.next_delay(), secs(1));
        let mut b = Backoff::new(secs(1), secs(30));
        for _ in 0..100 {
            b.next_delay();
        }
        assert_eq!(b.next_delay(), secs(30), "no overflow after many failures");
    }

    #[test]
    fn starts_then_becomes_healthy() {
        let (world, mut sup, t0) = setup();
        sup.tick(t0);
        assert_eq!(sup.state(), State::Starting);
        assert_eq!(world.borrow().starts, 1);
        sup.tick(t0 + secs(1));
        assert_eq!(sup.state(), State::Healthy);
        let snap = sup.snapshot(t0 + secs(1));
        assert_eq!(snap.engine_pid, Some(1001));
        assert_eq!(snap.restarts, 0);
    }

    #[test]
    fn a_killed_engine_is_restarted_with_backoff_one_two_four_eight() {
        let (world, mut sup, t0) = setup();
        let mut now = t0;
        sup.tick(now);
        sup.tick(now);
        assert_eq!(sup.state(), State::Healthy);
        for (round, expected) in [1u64, 2, 4, 8, 16, 30, 30].into_iter().enumerate() {
            world.borrow_mut().exit = Some(-1); // killed
            sup.tick(now);
            assert_eq!(sup.state(), State::Waiting, "round {round}");
            assert_eq!(sup.snapshot(now).next_start_s, Some(expected), "round {round}");
            let starts = world.borrow().starts;
            // Not before the delay ...
            sup.tick(now + Duration::from_millis(expected * 1000 - 1));
            assert_eq!(world.borrow().starts, starts, "round {round}: restarted early");
            // ... and at it.
            now += secs(expected);
            sup.tick(now);
            assert_eq!(world.borrow().starts, starts + 1, "round {round}");
            assert_eq!(sup.state(), State::Starting);
            sup.tick(now);
            assert_eq!(sup.state(), State::Healthy);
        }
        assert_eq!(sup.snapshot(now).restarts, 7);
        assert_eq!(sup.snapshot(now).last_exit, Some(-1));
    }

    #[test]
    fn sixty_healthy_seconds_reset_the_backoff() {
        let (world, mut sup, t0) = setup();
        sup.tick(t0);
        world.borrow_mut().exit = Some(1);
        sup.tick(t0);
        sup.tick(t0 + secs(1));
        world.borrow_mut().exit = Some(1);
        sup.tick(t0 + secs(1));
        assert_eq!(sup.snapshot(t0 + secs(1)).next_start_s, Some(2));
        let t = t0 + secs(3);
        sup.tick(t); // start
        sup.tick(t); // healthy since t
        sup.tick(t + secs(60)); // healthy for 60 s: reset
        world.borrow_mut().exit = Some(1);
        sup.tick(t + secs(61));
        assert_eq!(sup.snapshot(t + secs(61)).next_start_s, Some(1));
    }

    #[test]
    fn a_refusal_exit_code_is_not_retried() {
        let (world, mut sup, t0) = setup();
        sup.tick(t0);
        world.borrow_mut().exit = Some(EXIT_REFUSED);
        sup.tick(t0);
        assert_eq!(sup.state(), State::Refused);
        sup.tick(t0 + secs(3600));
        assert_eq!(world.borrow().starts, 1);
        assert!(sup.snapshot(t0).last_error.unwrap().contains("refused"));
        sup.resume();
        sup.tick(t0 + secs(3601));
        assert_eq!(world.borrow().starts, 2);
    }

    #[test]
    fn three_missed_probes_kill_and_restart() {
        let (world, mut sup, t0) = setup();
        sup.tick(t0);
        sup.tick(t0);
        world.borrow_mut().healthy = false;
        sup.tick(t0 + secs(2));
        sup.tick(t0 + secs(4));
        assert_eq!(sup.state(), State::Unhealthy);
        assert_eq!(world.borrow().kills, 0);
        sup.tick(t0 + secs(6));
        assert_eq!(world.borrow().kills, 1);
        assert_eq!(sup.state(), State::Waiting);
    }

    #[test]
    fn a_start_that_never_answers_times_out() {
        let (world, mut sup, t0) = setup();
        world.borrow_mut().healthy = false;
        sup.tick(t0);
        sup.tick(t0 + secs(599));
        assert_eq!(sup.state(), State::Starting);
        sup.tick(t0 + secs(600));
        assert_eq!(world.borrow().kills, 1);
        assert_eq!(sup.state(), State::Waiting);
    }

    #[test]
    fn a_failed_spawn_backs_off_too() {
        let (world, mut sup, t0) = setup();
        world.borrow_mut().fail_start = true;
        sup.tick(t0);
        assert_eq!(sup.state(), State::Waiting);
        assert_eq!(sup.snapshot(t0).next_start_s, Some(1));
        sup.tick(t0 + secs(1));
        assert_eq!(sup.snapshot(t0 + secs(1)).next_start_s, Some(2));
        assert!(sup.snapshot(t0).last_error.unwrap().starts_with("start: "));
    }

    #[test]
    fn stop_kills_the_engine_and_stays_stopped() {
        let (world, mut sup, t0) = setup();
        sup.tick(t0);
        sup.tick(t0);
        sup.stop();
        assert_eq!(world.borrow().kills, 1);
        assert_eq!(sup.state(), State::Stopped);
        sup.tick(t0 + secs(100));
        assert_eq!(world.borrow().starts, 1);
        assert_eq!(sup.snapshot(t0).engine_pid, None);
    }

    #[test]
    fn status_text_is_stable() {
        let text = render_status(&[
            ("state", "healthy".into()),
            ("engine_pid", "42".into()),
            ("last_exit", String::new()),
        ]);
        assert_eq!(text, "state: healthy\nengine_pid: 42\nlast_exit: none\n");
        assert_eq!(parse_status(&text)[2], ("last_exit".to_string(), "none".to_string()));
    }

    #[test]
    fn utc_timestamps() {
        assert_eq!(utc_timestamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc_timestamp(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(utc_timestamp(1_795_600_800), "2026-11-25T10:00:00Z");
    }
}
