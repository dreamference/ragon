//! macOS: the engine dies with its supervisor. macOS has no parent-death signal and no Job Object,
//! so the engine runs in a process group of its own (its pid is the group id) and, after each
//! start, the supervisor forks a guard: a small child that only waits, in kqueue, for either the
//! supervisor or the engine to exit (`EVFILT_PROC`, `NOTE_EXIT`).
//!
//! - The supervisor exits (a crash, `kill -9`): the guard sends `SIGKILL` to the engine's group (the
//!   engine and anything it started) and exits.
//! - The engine exits: the guard exits; the supervisor reaps it and forks a new one for the next start.
//!
//! The guard is a plain `fork()` without `exec`: between the fork and `_exit` it makes only system
//! calls (no allocation, no locks), so the supervisor's other threads cannot deadlock it. It closes
//! every inherited descriptor above stderr, so it holds no engine log or pipe open, and joins the
//! engine's group, so a `killpg` of that group (launchd stopping the job, a drill) takes it too.
//!
//! Hand-declared libSystem calls, the same way src/node_advertise_macos.rs in the launcher does,
//! so the crate needs no `libc` dependency.

use std::ffi::c_void;
use std::io;
use std::time::{Duration, Instant};

const EVFILT_PROC: i16 = -5;
const EV_ADD: u16 = 0x0001;
const EV_RECEIPT: u16 = 0x0040;
const EV_ERROR: u16 = 0x4000;
const NOTE_EXIT: u32 = 0x8000_0000;
const SIGKILL: i32 = 9;
const WNOHANG: i32 = 1;
const EINTR: i32 = 4;
/// Descriptors the guard closes: everything a supervisor plausibly has open above stderr.
const MAX_FD: i32 = 1024;

#[repr(C)]
struct KEvent {
    ident: usize,
    filter: i16,
    flags: u16,
    fflags: u32,
    data: isize,
    udata: *mut c_void,
}

unsafe extern "C" {
    fn fork() -> i32;
    fn _exit(code: i32) -> !;
    fn getppid() -> i32;
    fn setpgid(pid: i32, pgid: i32) -> i32;
    fn killpg(pgrp: i32, sig: i32) -> i32;
    fn kill(pid: i32, sig: i32) -> i32;
    fn close(fd: i32) -> i32;
    fn kqueue() -> i32;
    fn kevent(
        kq: i32,
        changelist: *const KEvent,
        nchanges: i32,
        eventlist: *mut KEvent,
        nevents: i32,
        timeout: *const c_void,
    ) -> i32;
    fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
    fn __error() -> *mut i32;
}

fn watch(pid: i32) -> KEvent {
    KEvent {
        ident: pid as usize,
        filter: EVFILT_PROC,
        flags: EV_ADD | EV_RECEIPT,
        fflags: NOTE_EXIT,
        data: 0,
        udata: std::ptr::null_mut(),
    }
}

/// The guard's body, in the forked child. Never returns.
///
/// # Safety
/// Only in a freshly forked child: system calls only, then `_exit`.
unsafe fn guard(supervisor: i32, engine: i32) -> ! {
    unsafe {
        for fd in 3..MAX_FD {
            close(fd);
        }
        // Into the engine's group; fails harmlessly when the engine is already gone.
        setpgid(0, engine);
        let changes = [watch(supervisor), watch(engine)];
        let mut receipts = [watch(0), watch(0)];
        let kq = kqueue();
        let registered = if kq >= 0 { kevent(kq, changes.as_ptr(), 2, receipts.as_mut_ptr(), 2, std::ptr::null()) } else { -1 };
        if registered != 2 {
            // No way to watch: the engine must not outlive an unwatched supervisor, but killing a
            // healthy engine here would turn every kqueue hiccup into a restart. Keep the old
            // behaviour (no guard) and let the supervisor reap us.
            _exit(1);
        }
        for r in &receipts {
            let failed = r.flags & EV_ERROR != 0 && r.data != 0;
            if failed && r.ident == supervisor as usize {
                killpg(engine, SIGKILL);
                _exit(0);
            }
            if failed && r.ident == engine as usize {
                _exit(0);
            }
        }
        // The supervisor died between the fork and the registration: we were re-parented.
        if getppid() != supervisor {
            killpg(engine, SIGKILL);
            _exit(0);
        }
        loop {
            let mut event = watch(0);
            let n = kevent(kq, std::ptr::null(), 0, &mut event, 1, std::ptr::null());
            if n < 0 && *__error() == EINTR {
                continue;
            }
            if n == 1 && event.ident == supervisor as usize {
                killpg(engine, SIGKILL);
            }
            _exit(0);
        }
    }
}

/// Forks the guard for `engine` (a group leader). The guard's pid, or the fork's error.
pub fn spawn(engine: u32) -> io::Result<i32> {
    let supervisor = std::process::id() as i32;
    let engine = engine as i32;
    // SAFETY: the child runs `guard`, which makes system calls only and ends in `_exit`.
    let pid = unsafe { fork() };
    match pid {
        -1 => Err(io::Error::last_os_error()),
        0 => unsafe { guard(supervisor, engine) },
        pid => Ok(pid),
    }
}

/// Reaps a guard whose engine has exited or been killed: it exits on its own within a moment;
/// after `grace` it is killed.
pub fn reap(guard: i32, grace: Duration) {
    let until = Instant::now() + grace;
    loop {
        let mut status = 0;
        // SAFETY: waitpid on our own child.
        let r = unsafe { waitpid(guard, &mut status, WNOHANG) };
        if r == guard || r < 0 {
            return;
        }
        if Instant::now() >= until {
            unsafe {
                kill(guard, SIGKILL);
                waitpid(guard, &mut status, 0);
            }
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// `SIGKILL` to the engine's whole group.
pub fn kill_group(engine: u32) -> io::Result<()> {
    // SAFETY: a plain system call.
    if unsafe { killpg(engine as i32, SIGKILL) } == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}
