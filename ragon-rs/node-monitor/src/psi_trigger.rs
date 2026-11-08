//! The kernel PSI trigger on /proc/pressure/memory (§3.5): `some 150000 2000000` is 150 ms stalled
//! within 2 s (an unprivileged process must use a window that is a multiple of 2 s). The service
//! polls the descriptor for `POLLPRI` and samples at once when it fires; the trigger never changes
//! a threshold, only when the next sample is taken.

use crate::host::Host;
use crate::{Item, State, Weight};
use std::io::{self, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::Path;

pub const TRIGGER: &str = "some 150000 2000000";

pub struct PsiTrigger {
    file: std::fs::File,
}

impl PsiTrigger {
    /// Opens `path` and registers the trigger. Fails when the kernel has no PSI, or refuses.
    pub fn arm(path: &Path) -> io::Result<PsiTrigger> {
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)?;
        // The kernel wants the string with its terminating NUL in one write.
        file.write_all(format!("{TRIGGER}\0").as_bytes())?;
        Ok(PsiTrigger { file })
    }

    /// Waits up to `timeout_ms` for the trigger to fire. `Ok(false)` is a timeout.
    pub fn wait(&self, timeout_ms: i32) -> io::Result<bool> {
        let mut fds = libc::pollfd {
            fd: self.file.as_raw_fd(),
            events: libc::POLLPRI,
            revents: 0,
        };
        // SAFETY: `fds` is one valid pollfd and the count says so.
        let n = unsafe { libc::poll(&mut fds, 1, timeout_ms) };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        if fds.revents & libc::POLLERR != 0 {
            return Err(io::Error::other("PSI trigger descriptor failed"));
        }
        Ok(fds.revents & libc::POLLPRI != 0)
    }

    pub fn into_fd(self) -> OwnedFd {
        self.file.into()
    }
}

/// The status item: whether the trigger can be armed here.
pub fn item(host: &dyn Host) -> Item {
    let name = "psi_trigger";
    match host.psi_trigger_armable() {
        Ok(()) => Item::new(name, State::Ok, Weight::Item, format!("armed: {TRIGGER}")),
        Err(e) => Item::new(
            name,
            State::Unknown,
            Weight::Item,
            format!("/proc/pressure/memory trigger: {}", e.kind()),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeHost;

    #[test]
    fn the_trigger_window_is_a_multiple_of_two_seconds() {
        let window: u64 = TRIGGER.split(' ').nth(2).unwrap().parse().unwrap();
        assert_eq!(window % 2_000_000, 0);
    }

    #[test]
    fn arming_writes_the_trigger_and_a_quiet_file_times_out() {
        let dir = std::env::temp_dir().join(format!("psi-trigger-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("memory");
        std::fs::write(&path, "").unwrap();
        let trigger = PsiTrigger::arm(&path).unwrap();
        assert_eq!(trigger.wait(0).unwrap(), false);
        drop(trigger);
        assert!(
            std::fs::read(&path)
                .unwrap()
                .starts_with(TRIGGER.as_bytes())
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_source_is_unknown_and_a_refusal_says_why() {
        assert!(PsiTrigger::arm(Path::new("/nonexistent/psi")).is_err());
        let none = item(&FakeHost::default());
        assert_eq!(none.state, State::Unknown);
        let denied = item(&FakeHost {
            psi_trigger: Some(false),
            ..Default::default()
        });
        assert!(denied.detail.contains("permission denied"));
        assert_eq!(
            item(&FakeHost {
                psi_trigger: Some(true),
                ..Default::default()
            })
            .state,
            State::Ok
        );
    }
}
