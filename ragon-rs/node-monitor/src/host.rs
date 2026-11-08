//! The only way the probes reach the machine: a file read, a command, a loopback HTTP GET and
//! `statvfs`. Tests implement `Host` with recorded bodies (`crate::fake`).

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// One HTTP answer from a loopback port.
#[derive(Clone, Debug, PartialEq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
    pub elapsed: Duration,
}

/// What a command printed.
#[derive(Clone, Debug, PartialEq)]
pub struct CmdOutput {
    pub success: bool,
    pub stdout: String,
}

/// What `statvfs` reports for a path. `avail` is `f_bavail`: what an unprivileged user can write.
#[derive(Clone, Debug, PartialEq)]
pub struct FsStat {
    pub fs_id: u64,
    pub total: u64,
    pub avail: u64,
}

pub trait Host {
    fn read_file(&self, path: &str) -> io::Result<String>;
    fn run(&self, program: &str, args: &[&str], timeout: Duration) -> io::Result<CmdOutput>;
    /// A GET on `127.0.0.1:port`. There is no way to name another host.
    fn http_get(&self, port: u16, path: &str, timeout: Duration) -> io::Result<HttpResponse>;
    /// A JSON POST on `127.0.0.1:port` (the liveness completion, §3.3).
    fn http_post(
        &self,
        _port: u16,
        _path: &str,
        _body: &str,
        _timeout: Duration,
    ) -> io::Result<HttpResponse> {
        Err(io::ErrorKind::Unsupported.into())
    }
    fn statvfs(&self, path: &str) -> io::Result<FsStat>;
    fn home(&self) -> Option<String>;
    /// The first `n` bytes of a file and the file's size.
    fn read_head(&self, path: &str, n: usize) -> io::Result<(Vec<u8>, u64)>;
    /// The names (not paths) in a directory, sorted.
    fn list_dir(&self, path: &str) -> io::Result<Vec<String>>;
    /// Seconds since the Unix epoch.
    fn now_unix(&self) -> u64;
    /// Whether a kernel PSI trigger can be armed on /proc/pressure/memory (§3.5).
    fn psi_trigger_armable(&self) -> io::Result<()>;
}

fn request(
    port: u16,
    method: &str,
    path: &str,
    body: &str,
    timeout: Duration,
) -> io::Result<HttpResponse> {
    let start = Instant::now();
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let extra = if method == "POST" {
        format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        )
    } else {
        String::new()
    };
    write!(
        stream,
        "{method} {path} HTTP/1.0\r\nHost: 127.0.0.1\r\n{extra}Connection: close\r\n\r\n{body}"
    )?;
    let mut raw = Vec::new();
    stream.take(4 << 20).read_to_end(&mut raw)?;
    parse_http_response(&raw, start.elapsed())
}

/// Splits a raw HTTP/1.0 answer into its status and body.
pub fn parse_http_response(raw: &[u8], elapsed: Duration) -> io::Result<HttpResponse> {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no header end"))?;
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no status line"))?;
    Ok(HttpResponse {
        status,
        body: body.to_string(),
        elapsed,
    })
}

/// The real machine. `root` is prefixed to every file read, so `--proc-root` can point at a copy.
pub struct RealHost {
    pub root: PathBuf,
}

impl Host for RealHost {
    fn read_file(&self, path: &str) -> io::Result<String> {
        std::fs::read_to_string(self.root.join(path.trim_start_matches('/')))
    }

    fn run(&self, program: &str, args: &[&str], timeout: Duration) -> io::Result<CmdOutput> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let start = Instant::now();
        loop {
            if let Some(status) = child.try_wait()? {
                let mut stdout = String::new();
                if let Some(mut pipe) = child.stdout.take() {
                    pipe.read_to_string(&mut stdout)?;
                }
                return Ok(CmdOutput {
                    success: status.success(),
                    stdout,
                });
            }
            if start.elapsed() >= timeout {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::new(io::ErrorKind::TimedOut, "command timed out"));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn http_get(&self, port: u16, path: &str, timeout: Duration) -> io::Result<HttpResponse> {
        request(port, "GET", path, "", timeout)
    }

    fn http_post(
        &self,
        port: u16,
        path: &str,
        body: &str,
        timeout: Duration,
    ) -> io::Result<HttpResponse> {
        request(port, "POST", path, body, timeout)
    }

    fn statvfs(&self, path: &str) -> io::Result<FsStat> {
        let c_path = std::ffi::CString::new(path)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "nul in path"))?;
        // SAFETY: `c_path` is a valid C string and `stat` is a zeroed plain-data struct that
        // statvfs fills in; the result is read only when the call returns 0.
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let block = stat.f_frsize as u64;
        let meta = std::fs::metadata(path)?;
        use std::os::unix::fs::MetadataExt;
        Ok(FsStat {
            fs_id: meta.dev(),
            total: stat.f_blocks as u64 * block,
            avail: stat.f_bavail as u64 * block,
        })
    }

    fn home(&self) -> Option<String> {
        std::env::var("HOME").ok()
    }

    fn read_head(&self, path: &str, n: usize) -> io::Result<(Vec<u8>, u64)> {
        let file = std::fs::File::open(path)?;
        let size = file.metadata()?.len();
        let mut buf = Vec::new();
        file.take(n as u64).read_to_end(&mut buf)?;
        Ok((buf, size))
    }

    fn list_dir(&self, path: &str) -> io::Result<Vec<String>> {
        let mut names: Vec<String> = std::fs::read_dir(path)?
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .collect();
        names.sort();
        Ok(names)
    }

    fn now_unix(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }

    fn psi_trigger_armable(&self) -> io::Result<()> {
        let path = self.root.join("proc/pressure/memory");
        crate::psi_trigger::PsiTrigger::arm(&path).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_status_and_body() {
        let raw = b"HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\n\r\nhello";
        let r = parse_http_response(raw, Duration::from_millis(3)).unwrap();
        assert_eq!((r.status, r.body.as_str()), (200, "hello"));
    }

    #[test]
    fn refuses_a_truncated_answer() {
        assert!(parse_http_response(b"HTTP/1.0 200 OK\r\n", Duration::ZERO).is_err());
        assert!(parse_http_response(b"garbage\r\n\r\n", Duration::ZERO).is_err());
    }
}
