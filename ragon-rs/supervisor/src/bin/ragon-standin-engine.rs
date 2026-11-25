//! A stand-in for the model server, for the supervisor's tests and drills: it answers
//! `GET /v1/models` with a one-model list and 404 to anything else, and can be killed on demand
//! (by pid, or `GET /exit` makes it exit with `--exit-code`).
//!
//! `ragon-standin-engine --port P [--pid-file F] [--exit-code N] [--exit-after-ms MS] [--refuse]`
//! `--refuse` exits 3 at once (the "do not retry" code); `--exit-after-ms` exits on its own.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut port = 0u16;
    let mut pid_file = None;
    let mut exit_code = 1i32;
    let mut exit_after_ms = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let value = it.clone().next().cloned().unwrap_or_default();
        match arg.as_str() {
            "--port" => port = value.parse().expect("--port needs a number"),
            "--pid-file" => pid_file = Some(value),
            "--exit-code" => exit_code = value.parse().expect("--exit-code needs a number"),
            "--exit-after-ms" => exit_after_ms = Some(value.parse::<u64>().expect("--exit-after-ms needs a number")),
            "--refuse" => std::process::exit(3),
            other => panic!("unknown argument {other}"),
        }
        it.next();
    }
    if let Some(path) = pid_file {
        std::fs::write(path, format!("{}\n", std::process::id())).expect("pid file");
    }
    if let Some(ms) = exit_after_ms {
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(ms));
            std::process::exit(exit_code);
        });
    }
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("bind");
    eprintln!("standin: listening on {} pid {}", listener.local_addr().unwrap(), std::process::id());
    for stream in listener.incoming().flatten() {
        if handle(stream) {
            std::process::exit(exit_code);
        }
    }
}

/// Answers one request; true when it asked the stand-in to exit.
fn handle(mut stream: TcpStream) -> bool {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut buf = [0u8; 2048];
    let n = stream.read(&mut buf).unwrap_or(0);
    let request = String::from_utf8_lossy(&buf[..n]);
    let path = request.split_whitespace().nth(1).unwrap_or("");
    let (status, body, exit) = match path {
        "/v1/models" => (
            "200 OK",
            r#"{"object":"list","data":[{"id":"standin","object":"model","owned_by":"ragon"}]}"#,
            false,
        ),
        "/exit" => ("200 OK", r#"{"exiting":true}"#, true),
        _ => ("404 Not Found", r#"{"error":"not found"}"#, false),
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
    exit
}
