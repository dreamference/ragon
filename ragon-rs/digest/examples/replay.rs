//! Replays outputs through the digest, for scoring it against a replay set outside the repository.
//!
//! `replay <out_dir> [--last-good] [--min-lines N] [--model http://HOST:PORT --model-name NAME]`
//! reads one case per line on stdin, tab-separated: the case id, the raw output's path, the exit
//! code (`-` while running), the command and, optionally, the path of the same command's last
//! successful output. Each case gets a fresh home; what the model would be given is written to
//! `<out_dir>/<id>.out`. With `--model`, the model step asks that OpenAI-compatible server (plain
//! HTTP, thinking off, 20 s), and each case's journal lines go to `<out_dir>/<id>.journal`.

use std::io::{BufRead, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::pin::pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use ragon_digest::model::{Reply, Selector};
use ragon_digest::{Digester, Options};

/// The model server's generated tokens, summed over the run.
static GENERATED: AtomicU64 = AtomicU64::new(0);

fn value<'a>(args: &'a [String], name: &str) -> Option<&'a String> {
    args.iter().position(|arg| arg == name).and_then(|at| args.get(at + 1))
}

/// A JSON string literal.
fn json(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The JSON string that starts at `text` (just after its opening quote).
fn unquote(text: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                'u' => {
                    let code: String = chars.by_ref().take(4).collect();
                    out.push(char::from_u32(u32::from_str_radix(&code, 16).ok()?).unwrap_or('\u{fffd}'));
                }
                other => out.push(other),
            },
            c => out.push(c),
        }
    }
    None
}

struct Server {
    address: String,
    name: String,
}

impl Server {
    fn post(&self, prompt: &str) -> Result<String, String> {
        let body = format!(
            "{{\"model\":{},\"messages\":[{{\"role\":\"user\",\"content\":{}}}],\"temperature\":0,\"max_tokens\":400,\"chat_template_kwargs\":{{\"enable_thinking\":false}}}}",
            json(&self.name),
            json(prompt)
        );
        let mut stream = TcpStream::connect(&self.address).map_err(|error| error.to_string())?;
        stream.set_read_timeout(Some(Duration::from_secs(20))).map_err(|error| error.to_string())?;
        write!(
            stream,
            "POST /v1/chat/completions HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            self.address,
            body.len()
        )
        .map_err(|error| error.to_string())?;
        let mut response = String::new();
        stream.read_to_string(&mut response).map_err(|error| error.to_string())?;
        if let Some(at) = response.find("\"completion_tokens\":") {
            let digits: String =
                response[at + 20..].chars().skip_while(|c| *c == ' ').take_while(char::is_ascii_digit).collect();
            GENERATED.fetch_add(digits.parse().unwrap_or(0), Ordering::Relaxed);
        }
        let at = response.find("\"content\":\"").ok_or_else(|| format!("no content: {}", &response[..response.len().min(200)]))?;
        unquote(&response[at + "\"content\":\"".len()..]).ok_or_else(|| "unterminated content".to_string())
    }
}

impl Selector for Server {
    fn ask<'a>(&'a self, prompt: &'a str) -> Reply<'a> {
        let reply = self.post(prompt);
        Box::pin(async move { reply })
    }
}

/// Runs a future that never waits (the server is asked synchronously).
fn ready<T>(future: impl Future<Output = T>) -> T {
    let mut future = pin!(future);
    match future.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("the replay's model call never waits"),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out_dir = PathBuf::from(args.first().expect("usage: replay <out_dir> [--last-good]"));
    let last_good = args.iter().any(|arg| arg == "--last-good");
    let min_lines = value(&args, "--min-lines").map_or(ragon_digest::MIN_LINES, |n| n.parse().expect("--min-lines N"));
    let server = value(&args, "--model").map(|url| Server {
        address: url.trim_start_matches("http://").trim_end_matches('/').to_string(),
        name: value(&args, "--model-name").cloned().unwrap_or_default(),
    });
    let model = server.is_some();
    std::fs::create_dir_all(&out_dir).expect("out_dir");
    for line in std::io::stdin().lock().lines() {
        let line = line.expect("stdin");
        let fields: Vec<&str> = line.split('\t').collect();
        let [id, raw, exit, command, rest @ ..] = fields.as_slice() else { continue };
        let home = std::env::temp_dir().join(format!("ragon-digest-replay-{}-{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let mut digester = Digester::with_options(&home, Options { last_good, min_lines, model });
        if let Some(server) = &server {
            digester = digester.with_selector(Box::new(Server { address: server.address.clone(), name: server.name.clone() }));
        }
        if let Some(good) = rest.first().filter(|path| !path.is_empty()) {
            digester.process(command, std::fs::read(good).expect("good run"), Some(0));
        }
        let exit_code = exit.parse::<i32>().ok();
        let output = ready(digester.process_async(command, std::fs::read(raw).expect("raw"), exit_code));
        std::fs::write(out_dir.join(format!("{id}.out")), output).expect("write");
        if let Ok(journal) = std::fs::read(digester.journal_path()) {
            std::fs::write(out_dir.join(format!("{id}.journal")), journal).expect("write");
        }
        let _ = std::fs::remove_dir_all(&home);
    }
    if model {
        eprintln!("generated tokens: {}", GENERATED.load(Ordering::Relaxed));
    }
}
