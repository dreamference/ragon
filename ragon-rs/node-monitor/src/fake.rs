//! A scripted `Host` for the tests: files, commands, HTTP answers and filesystems are tables, and
//! anything not in a table fails like a missing thing on a real machine.

use crate::host::{CmdOutput, FsStat, Host, HttpResponse};
use std::collections::HashMap;
use std::io;
use std::time::Duration;

#[derive(Default)]
pub struct FakeHost {
    pub files: HashMap<String, String>,
    /// Keyed by the command line joined with spaces.
    pub commands: HashMap<String, CmdOutput>,
    /// Keyed by `port path`.
    pub http: HashMap<String, HttpResponse>,
    pub fs: HashMap<String, FsStat>,
    pub home: Option<String>,
    /// Keyed by `port path`; a POST that finds nothing fails like a refused connection.
    pub posts: HashMap<String, HttpResponse>,
    /// Every POST made, as `port path`.
    pub posted: std::cell::RefCell<Vec<String>>,
    /// Keyed by path: the file's bytes (read_head cuts them) and its reported size.
    pub heads: HashMap<String, (Vec<u8>, u64)>,
    pub dirs: HashMap<String, Vec<String>>,
    pub now: u64,
    /// `Some(true)` arms, `Some(false)` is refused, `None` has no PSI.
    pub psi_trigger: Option<bool>,
}

impl FakeHost {
    pub fn head(mut self, path: &str, bytes: Vec<u8>, size: u64) -> Self {
        self.heads.insert(path.into(), (bytes, size));
        self
    }
    pub fn dir(mut self, path: &str, names: &[&str]) -> Self {
        self.dirs
            .insert(path.into(), names.iter().map(|n| n.to_string()).collect());
        self
    }
    pub fn file(mut self, path: &str, text: &str) -> Self {
        self.files.insert(path.into(), text.into());
        self
    }
    pub fn answer(mut self, port: u16, path: &str, status: u16, ms: u64, body: &str) -> Self {
        let r = HttpResponse {
            status,
            body: body.into(),
            elapsed: Duration::from_millis(ms),
        };
        self.http.insert(format!("{port} {path}"), r);
        self
    }
    pub fn post_answer(mut self, port: u16, path: &str, status: u16) -> Self {
        let r = HttpResponse {
            status,
            body: String::new(),
            elapsed: Duration::from_millis(50),
        };
        self.posts.insert(format!("{port} {path}"), r);
        self
    }
    pub fn command(mut self, line: &str, stdout: &str) -> Self {
        self.commands.insert(
            line.into(),
            CmdOutput {
                success: true,
                stdout: stdout.into(),
            },
        );
        self
    }
    pub fn volume(mut self, path: &str, fs_id: u64, total_gib: u64, avail_gib: u64) -> Self {
        let gib = 1u64 << 30;
        self.fs.insert(
            path.into(),
            FsStat {
                fs_id,
                total: total_gib * gib,
                avail: avail_gib * gib,
            },
        );
        self
    }
}

impl Host for FakeHost {
    fn read_file(&self, path: &str) -> io::Result<String> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| io::ErrorKind::NotFound.into())
    }
    fn run(&self, program: &str, args: &[&str], _t: Duration) -> io::Result<CmdOutput> {
        let line = std::iter::once(program)
            .chain(args.iter().copied())
            .collect::<Vec<_>>()
            .join(" ");
        self.commands
            .get(&line)
            .cloned()
            .ok_or_else(|| io::ErrorKind::NotFound.into())
    }
    fn http_get(&self, port: u16, path: &str, _t: Duration) -> io::Result<HttpResponse> {
        self.http
            .get(&format!("{port} {path}"))
            .cloned()
            .ok_or_else(|| io::ErrorKind::ConnectionRefused.into())
    }
    fn http_post(&self, port: u16, path: &str, _b: &str, _t: Duration) -> io::Result<HttpResponse> {
        let key = format!("{port} {path}");
        self.posted.borrow_mut().push(key.clone());
        self.posts
            .get(&key)
            .cloned()
            .ok_or_else(|| io::ErrorKind::ConnectionRefused.into())
    }
    fn statvfs(&self, path: &str) -> io::Result<FsStat> {
        self.fs
            .get(path)
            .cloned()
            .ok_or_else(|| io::ErrorKind::NotFound.into())
    }
    fn home(&self) -> Option<String> {
        self.home.clone()
    }
    fn read_head(&self, path: &str, n: usize) -> io::Result<(Vec<u8>, u64)> {
        let (bytes, size) = self
            .heads
            .get(path)
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        Ok((bytes[..n.min(bytes.len())].to_vec(), *size))
    }
    fn list_dir(&self, path: &str) -> io::Result<Vec<String>> {
        self.dirs
            .get(path)
            .cloned()
            .ok_or_else(|| io::ErrorKind::NotFound.into())
    }
    fn now_unix(&self) -> u64 {
        self.now
    }
    fn psi_trigger_armable(&self) -> io::Result<()> {
        match self.psi_trigger {
            Some(true) => Ok(()),
            Some(false) => Err(io::ErrorKind::PermissionDenied.into()),
            None => Err(io::ErrorKind::NotFound.into()),
        }
    }
}
