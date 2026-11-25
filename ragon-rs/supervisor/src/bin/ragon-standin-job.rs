//! A stand-in job for the job drills (tests/jobs.rs) and the Mac sandbox probes: the fake `ragon`
//! the supervisor runs. One mode per run; it prints what it did and exits 0 when the action
//! succeeded, 1 when it was refused (the sandbox probes read that), 2 on bad arguments.
//!
//! - `hog <MiB> <seconds>`: touches MiB of memory, then sleeps
//! - `sleep <seconds>`
//! - `stubborn <seconds>`: ignores SIGTERM, then sleeps (only SIGKILL stops it)
//! - `spawn <seconds>`: starts a child of itself (`sleep`) and sleeps (a group with two members)
//! - `read <path>`, `write <path>`, `list <dir>`
//! - `connect <host:port>`, `connect-unix <path>`, `resolve <host>`
//! - `print <text>`

use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

unsafe extern "C" {
    fn signal(sig: i32, handler: usize) -> usize;
}
const SIGTERM: i32 = 15;
const SIG_IGN: usize = 1;

fn seconds(a: Option<&String>) -> Duration {
    Duration::from_secs(a.and_then(|s| s.parse().ok()).unwrap_or(3600))
}

fn report<T, E: std::fmt::Display>(what: &str, r: Result<T, E>) -> i32 {
    match r {
        Ok(_) => {
            println!("allowed: {what}");
            0
        }
        Err(e) => {
            println!("denied: {what}: {e}");
            1
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mode, arg, arg2) = (args.first().map(String::as_str), args.get(1), args.get(2));
    let code = match (mode, arg) {
        (Some("hog"), Some(mib)) => {
            let Ok(mib) = mib.parse::<usize>() else { std::process::exit(2) };
            let mut block = vec![0u8; mib << 20];
            // Touch every page so it is resident (and counted in the footprint).
            for i in (0..block.len()).step_by(4096) {
                block[i] = (i % 251) as u8 + 1;
            }
            println!("holding {mib} MiB");
            let _ = std::io::stdout().flush();
            std::thread::sleep(seconds(arg2));
            std::hint::black_box(&block);
            0
        }
        (Some("sleep"), a) => {
            println!("sleeping");
            let _ = std::io::stdout().flush();
            std::thread::sleep(seconds(a));
            0
        }
        (Some("stubborn"), a) => {
            // SAFETY: installs SIG_IGN for SIGTERM; no handler code runs.
            unsafe { signal(SIGTERM, SIG_IGN) };
            println!("ignoring SIGTERM");
            let _ = std::io::stdout().flush();
            std::thread::sleep(seconds(a));
            0
        }
        (Some("spawn"), a) => {
            let me = std::env::current_exe().expect("own path");
            let child = std::process::Command::new(me).arg("sleep").args(a).spawn();
            match child {
                Ok(c) => println!("child {}", c.id()),
                Err(e) => println!("no child: {e}"),
            }
            let _ = std::io::stdout().flush();
            std::thread::sleep(seconds(a));
            0
        }
        (Some("read"), Some(p)) => report(&format!("read {p}"), std::fs::read(p)),
        (Some("list"), Some(p)) => report(&format!("list {p}"), std::fs::read_dir(p)),
        (Some("write"), Some(p)) => report(&format!("write {p}"), std::fs::write(p, b"standin\n")),
        (Some("connect"), Some(addr)) => report(
            &format!("connect {addr}"),
            addr.to_socket_addrs()
                .and_then(|mut a| a.next().ok_or_else(|| std::io::Error::other("no address")))
                .and_then(|a| TcpStream::connect_timeout(&a, Duration::from_secs(3))),
        ),
        #[cfg(unix)]
        (Some("connect-unix"), Some(p)) => {
            report(&format!("connect-unix {p}"), std::os::unix::net::UnixStream::connect(p))
        }
        (Some("resolve"), Some(host)) => report(
            &format!("resolve {host}"),
            (host.as_str(), 443)
                .to_socket_addrs()
                .and_then(|mut a| a.next().ok_or_else(|| std::io::Error::other("no address"))),
        ),
        (Some("print"), Some(text)) => {
            println!("{text}");
            0
        }
        _ => {
            eprintln!("usage: ragon-standin-job <hog|sleep|stubborn|spawn|read|write|list|connect|connect-unix|resolve|print> ...");
            2
        }
    };
    std::process::exit(code);
}
