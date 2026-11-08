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
}

impl FakeHost {
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
    fn statvfs(&self, path: &str) -> io::Result<FsStat> {
        self.fs
            .get(path)
            .cloned()
            .ok_or_else(|| io::ErrorKind::NotFound.into())
    }
    fn home(&self) -> Option<String> {
        self.home.clone()
    }
}
