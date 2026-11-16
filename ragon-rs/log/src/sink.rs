//! Where lines go (§3.1) and how the file rotates (§6.1).

use crate::level::Level;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// A destination for finished lines. `write` never panics and never retries in a loop.
pub trait Sink: Send {
    /// Writes one line (no newline). Returns false when the line was dropped.
    fn write(&mut self, level: Level, line: &str, now_ms: u64) -> bool;
    /// Lines dropped and not yet reported, with a reason enum, once the sink can write again.
    fn pending_drop(&self, _now_ms: u64) -> Option<(u64, &'static str)> {
        None
    }
    /// Called after the report of `pending_drop` was written.
    fn clear_drop(&mut self) {}
}

/// Standard error as is: a container, where Docker captures it.
pub struct StderrSink;

impl Sink for StderrSink {
    fn write(&mut self, _level: Level, line: &str, _now_ms: u64) -> bool {
        writeln!(io::stderr(), "{line}").is_ok()
    }
}

/// Standard error with the syslog priority prefix, under journald.
pub struct JournaldSink;

impl Sink for JournaldSink {
    fn write(&mut self, level: Level, line: &str, _now_ms: u64) -> bool {
        writeln!(io::stderr(), "{}{line}", level.syslog_prefix()).is_ok()
    }
}

/// True when `JOURNAL_STREAM` (`dev:inode`) names the stream standard error is.
pub fn under_journald() -> bool {
    let Ok(spec) = std::env::var("JOURNAL_STREAM") else { return false };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let Some((dev, ino)) = spec.split_once(':') else { return false };
        return fs::metadata("/proc/self/fd/2")
            .or_else(|_| fs::metadata("/dev/stderr"))
            .is_ok_and(|m| m.dev().to_string() == dev && m.ino().to_string() == ino);
    }
    #[allow(unreachable_code)]
    {
        let _ = spec;
        false
    }
}

/// Rotation limits (§6.1).
#[derive(Clone, Copy, Debug)]
pub struct Rotation {
    pub max_bytes: u64,
    pub keep: usize,
    pub max_age: Duration,
}

impl Default for Rotation {
    fn default() -> Self {
        Rotation { max_bytes: 10 * 1024 * 1024, keep: 5, max_age: Duration::from_secs(14 * 86_400) }
    }
}

#[cfg(unix)]
fn file_id(m: &fs::Metadata) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (m.dev(), m.ino())
}
#[cfg(not(unix))]
fn file_id(_m: &fs::Metadata) -> (u64, u64) {
    (0, 0)
}

/// `<dir>/<component>.log`, appended to with `O_APPEND`, rotated by the writing process.
pub struct FileSink {
    dir: PathBuf,
    component: String,
    rotation: Rotation,
    file: Option<(File, (u64, u64))>,
    last_check_ms: u64,
    retry_at_ms: u64,
    dropped: u64,
    drop_reason: &'static str,
}

impl FileSink {
    pub fn new(dir: &Path, component: &str, rotation: Rotation) -> Self {
        FileSink {
            dir: dir.to_path_buf(),
            component: component.to_string(),
            rotation,
            file: None,
            last_check_ms: 0,
            retry_at_ms: 0,
            dropped: 0,
            drop_reason: "io",
        }
    }

    fn path(&self) -> PathBuf {
        self.dir.join(format!("{}.log", self.component))
    }

    fn open(&mut self) -> io::Result<()> {
        make_dir(&self.dir)?;
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(self.path())?;
        let id = file_id(&file.metadata()?);
        self.file = Some((file, id));
        Ok(())
    }

    /// Once a second: reopen when the path names another file, and rotate past the size.
    fn check(&mut self) -> io::Result<()> {
        let on_disk = fs::metadata(self.path()).ok();
        let stale = match (&self.file, &on_disk) {
            (Some((_, id)), Some(m)) => file_id(m) != *id,
            _ => true,
        };
        if stale {
            self.open()?;
        }
        let len = self.file.as_ref().map(|(f, _)| f.metadata().map(|m| m.len())).transpose()?.unwrap_or(0);
        if len >= self.rotation.max_bytes {
            self.rotate()?;
            self.open()?;
        }
        Ok(())
    }

    fn numbered(&self, n: usize, gz: bool) -> PathBuf {
        self.dir.join(format!("{}.{n}.log{}", self.component, if gz { ".gz" } else { "" }))
    }

    /// Shifts the files under the advisory lock; a process that lost the race finds a small file and
    /// does nothing. `.1.log` stays plain until the next rotation compresses it to `.2.log.gz`.
    fn rotate(&mut self) -> io::Result<()> {
        let lock =
            OpenOptions::new().create(true).append(true).open(self.dir.join(format!("{}.lock", self.component)))?;
        lock.lock()?;
        let result = (|| {
            let current = self.path();
            if fs::metadata(&current).map_or(true, |m| m.len() < self.rotation.max_bytes) {
                return Ok(());
            }
            let last = self.rotation.keep - 1;
            let _ = fs::remove_file(self.numbered(last, true));
            for n in (2..last).rev() {
                let _ = fs::rename(self.numbered(n, true), self.numbered(n + 1, true));
            }
            if self.numbered(1, false).exists() {
                compress(&self.numbered(1, false), &self.numbered(2, true))?;
            }
            fs::rename(&current, self.numbered(1, false))?;
            self.delete_old();
            Ok(())
        })();
        let _ = lock.unlock();
        result
    }

    fn delete_old(&self) {
        let prefix = format!("{}.", self.component);
        let Ok(entries) = fs::read_dir(&self.dir) else { return };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let rotated = name.starts_with(&prefix) && (name.ends_with(".log") || name.ends_with(".log.gz"));
            let old = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| SystemTime::now().duration_since(t).ok())
                .is_some_and(|age| age > self.rotation.max_age);
            if rotated && old {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

fn make_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

fn compress(from: &Path, to: &Path) -> io::Result<()> {
    let mut input = File::open(from)?;
    let mut out = flate2::write::GzEncoder::new(File::create(to)?, flate2::Compression::default());
    io::copy(&mut input, &mut out)?;
    out.finish()?;
    fs::remove_file(from)
}

impl Sink for FileSink {
    fn write(&mut self, _level: Level, line: &str, now_ms: u64) -> bool {
        if now_ms < self.retry_at_ms {
            self.dropped += 1;
            return false;
        }
        let attempt = (|| {
            if self.file.is_none() || now_ms >= self.last_check_ms + 1000 {
                self.last_check_ms = now_ms;
                self.check()?;
            }
            let (file, _) = self.file.as_mut().ok_or_else(|| io::Error::other("no file"))?;
            file.write_all(format!("{line}\n").as_bytes())
        })();
        match attempt {
            Ok(()) => true,
            Err(_) => {
                self.file = None;
                self.dropped += 1;
                self.retry_at_ms = now_ms + 1000;
                false
            }
        }
    }

    fn pending_drop(&self, now_ms: u64) -> Option<(u64, &'static str)> {
        (self.dropped > 0 && now_ms >= self.retry_at_ms).then_some((self.dropped, self.drop_reason))
    }

    fn clear_drop(&mut self) {
        self.dropped = 0;
    }
}
