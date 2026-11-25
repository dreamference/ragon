//! The health probe: a plain HTTP/1.1 `GET /v1/models` over a loopback TCP connection. A 200
//! answer within the timeout is healthy; anything else (refused, timeout, 401, 500) is not.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::core::Probe;

pub const MODELS_PATH: &str = "/v1/models";

#[derive(Clone, Debug)]
pub struct HttpProbe {
    pub host: String,
    pub port: u16,
    pub path: String,
    pub timeout: Duration,
}

impl HttpProbe {
    pub fn models(host: &str, port: u16) -> Self {
        Self { host: host.to_string(), port, path: MODELS_PATH.to_string(), timeout: Duration::from_secs(3) }
    }

    pub fn url(&self) -> String {
        format!("http://{}:{}{}", self.host, self.port, self.path)
    }

    /// The HTTP status code, or `None` when nothing answered.
    pub fn status_code(&self) -> Option<u16> {
        let addr: SocketAddr = (self.host.as_str(), self.port).to_socket_addrs().ok()?.next()?;
        let mut stream = TcpStream::connect_timeout(&addr, self.timeout).ok()?;
        stream.set_read_timeout(Some(self.timeout)).ok()?;
        stream.set_write_timeout(Some(self.timeout)).ok()?;
        let request = format!(
            "GET {} HTTP/1.1\r\nHost: {}:{}\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
            self.path, self.host, self.port
        );
        stream.write_all(request.as_bytes()).ok()?;
        let mut head = [0u8; 64];
        let mut len = 0;
        while len < head.len() {
            match stream.read(&mut head[len..]) {
                Ok(0) => break,
                Ok(n) => {
                    len += n;
                    if head[..len].contains(&b'\n') {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        parse_status_line(&head[..len])
    }
}

impl Probe for HttpProbe {
    fn healthy(&mut self) -> bool {
        self.status_code() == Some(200)
    }
}

/// `HTTP/1.1 200 OK` -> 200.
pub fn parse_status_line(head: &[u8]) -> Option<u16> {
    let line = std::str::from_utf8(head).ok()?.lines().next()?;
    let mut parts = line.split_whitespace();
    if !parts.next()?.starts_with("HTTP/") {
        return None;
    }
    parts.next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn status_lines() {
        assert_eq!(parse_status_line(b"HTTP/1.1 200 OK\r\n"), Some(200));
        assert_eq!(parse_status_line(b"HTTP/1.0 401 Unauthorized\r\n"), Some(401));
        assert_eq!(parse_status_line(b"garbage"), None);
        assert_eq!(parse_status_line(b""), None);
    }

    #[test]
    fn nothing_listening_is_unhealthy() {
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let mut probe = HttpProbe::models("127.0.0.1", port);
        probe.timeout = Duration::from_millis(500);
        assert!(!probe.healthy());
    }

    #[test]
    fn a_200_is_healthy_and_a_401_is_not() {
        for (answer, healthy) in [("200 OK", true), ("401 Unauthorized", false)] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let server = std::thread::spawn(move || {
                let (mut s, _) = listener.accept().unwrap();
                let mut buf = [0u8; 512];
                let n = s.read(&mut buf).unwrap();
                assert!(std::str::from_utf8(&buf[..n]).unwrap().starts_with("GET /v1/models HTTP/1.1\r\n"));
                write!(s, "HTTP/1.1 {answer}\r\nContent-Length: 0\r\n\r\n").unwrap();
            });
            assert_eq!(HttpProbe::models("127.0.0.1", port).healthy(), healthy);
            server.join().unwrap();
        }
    }
}
