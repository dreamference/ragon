//! A job's processes and their memory, for the job runner (src/jobs.rs; specs/DREAMFERENCE_MAC_NODE.md
//! §4.5). macOS ignores `RLIMIT_AS` and `RLIMIT_RSS`, so the supervisor samples the physical
//! footprint (`proc_pid_rusage`, `ri_phys_footprint`: what Activity Monitor calls Memory, which
//! counts compressed and swapped pages too) of every process in the job's group and below its
//! leader, and kills the group over the cap. On Linux, where the tests also run, the sample is
//! the resident set (`VmRSS`) of the same processes.
//!
//! Hand-written declarations of the few C functions used, so the crate needs no `libc`.

use std::collections::BTreeSet;

pub const SIGTERM: i32 = 15;
pub const SIGKILL: i32 = 9;

unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

/// Sends `sig` to the process group `pgid` and to every process in `pids` (a child that left the
/// group with `setpgid` is still caught by its parentage).
pub fn signal_job(pgid: i32, pids: &BTreeSet<i32>, sig: i32) {
    if pgid > 1 {
        // SAFETY: plain system call; a negative pid addresses the group.
        unsafe { kill(-pgid, sig) };
    }
    for &pid in pids {
        if pid > 1 {
            // SAFETY: as above.
            unsafe { kill(pid, sig) };
        }
    }
}

/// How the runner measures a job; the tests substitute their own.
pub trait Footprint: Send + Sync {
    /// The job's processes: the members of group `pgid` and every descendant of `leader`.
    fn pids(&self, pgid: i32, leader: i32) -> BTreeSet<i32>;
    /// The memory of one process in bytes, 0 when it is gone.
    fn bytes(&self, pid: i32) -> u64;
    /// The sum over [`Footprint::pids`].
    fn total(&self, pgid: i32, leader: i32) -> u64 {
        self.pids(pgid, leader).into_iter().map(|p| self.bytes(p)).sum()
    }
}

/// The real measurement of this platform.
pub struct SystemFootprint;

fn descendants(roots: &BTreeSet<i32>, mut children_of: impl FnMut(i32) -> Vec<i32>) -> BTreeSet<i32> {
    let mut all = roots.clone();
    let mut queue: Vec<i32> = roots.iter().copied().collect();
    while let Some(pid) = queue.pop() {
        for child in children_of(pid) {
            if all.insert(child) {
                queue.push(child);
            }
        }
    }
    all
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::c_void;

    const PROC_PGRP_ONLY: u32 = 2;
    const PROC_PPID_ONLY: u32 = 6;
    const RUSAGE_INFO_V0: i32 = 0;
    /// `ri_phys_footprint`'s index in `rusage_info_v0` read as `u64`s: 16 bytes of uuid, then
    /// user time, system time, idle wake-ups, interrupt wake-ups, page-ins, wired, resident.
    const PHYS_FOOTPRINT_INDEX: usize = 9;

    unsafe extern "C" {
        fn proc_listpids(kind: u32, typeinfo: u32, buffer: *mut c_void, buffersize: i32) -> i32;
        fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut c_void) -> i32;
    }

    pub fn list(kind_is_pgrp: bool, id: i32) -> Vec<i32> {
        let kind = if kind_is_pgrp { PROC_PGRP_ONLY } else { PROC_PPID_ONLY };
        let mut buf = vec![0i32; 4096];
        // SAFETY: the buffer is valid for its stated size in bytes.
        let bytes = unsafe {
            proc_listpids(kind, id as u32, buf.as_mut_ptr().cast(), (buf.len() * 4) as i32)
        };
        if bytes <= 0 {
            return Vec::new();
        }
        buf.truncate((bytes as usize / 4).min(buf.len()));
        buf.retain(|&p| p > 0);
        buf
    }

    pub fn footprint(pid: i32) -> u64 {
        // Larger than any rusage_info version, zeroed.
        let mut info = [0u64; 64];
        // SAFETY: the buffer outsizes rusage_info_v0.
        let rc = unsafe { proc_pid_rusage(pid, RUSAGE_INFO_V0, info.as_mut_ptr().cast()) };
        if rc == 0 { info[PHYS_FOOTPRINT_INDEX] } else { 0 }
    }
}

#[cfg(target_os = "macos")]
impl Footprint for SystemFootprint {
    fn pids(&self, pgid: i32, leader: i32) -> BTreeSet<i32> {
        let mut roots: BTreeSet<i32> = mac::list(true, pgid).into_iter().collect();
        if leader > 0 {
            roots.insert(leader);
        }
        descendants(&roots, |pid| mac::list(false, pid))
    }
    fn bytes(&self, pid: i32) -> u64 {
        mac::footprint(pid)
    }
}

#[cfg(not(target_os = "macos"))]
mod procfs {
    /// (pid, ppid, pgrp) of every process, from /proc/<pid>/stat.
    pub fn table() -> Vec<(i32, i32, i32)> {
        let Ok(dir) = std::fs::read_dir("/proc") else { return Vec::new() };
        dir.filter_map(|e| {
            let pid: i32 = e.ok()?.file_name().to_str()?.parse().ok()?;
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
            // The command name is in parentheses and may contain spaces.
            let rest = &stat[stat.rfind(')')? + 2..];
            let mut f = rest.split(' ');
            let _state = f.next()?;
            let ppid = f.next()?.parse().ok()?;
            let pgrp = f.next()?.parse().ok()?;
            Some((pid, ppid, pgrp))
        })
        .collect()
    }

    pub fn rss(pid: i32) -> u64 {
        let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/status")) else { return 0 };
        status
            .lines()
            .find_map(|l| l.strip_prefix("VmRSS:"))
            .and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
            .map_or(0, |kb| kb * 1024)
    }
}

#[cfg(not(target_os = "macos"))]
impl Footprint for SystemFootprint {
    fn pids(&self, pgid: i32, leader: i32) -> BTreeSet<i32> {
        let table = procfs::table();
        let mut roots: BTreeSet<i32> = table.iter().filter(|t| t.2 == pgid).map(|t| t.0).collect();
        if leader > 0 && table.iter().any(|t| t.0 == leader) {
            roots.insert(leader);
        }
        descendants(&roots, |pid| table.iter().filter(|t| t.1 == pid).map(|t| t.0).collect())
    }
    fn bytes(&self, pid: i32) -> u64 {
        procfs::rss(pid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descendants_follow_parentage_once() {
        let tree = |p: i32| match p {
            10 => vec![11, 12],
            11 => vec![13],
            13 => vec![10], // a cycle must not loop
            _ => vec![],
        };
        let roots: BTreeSet<i32> = [10].into_iter().collect();
        assert_eq!(descendants(&roots, tree).into_iter().collect::<Vec<_>>(), vec![10, 11, 12, 13]);
    }

    #[test]
    fn this_process_is_measured() {
        let me = std::process::id() as i32;
        assert!(SystemFootprint.bytes(me) > 0);
        assert!(SystemFootprint.pids(0, me).contains(&me));
    }
}
