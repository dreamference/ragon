//! Replays outputs through the digest, for scoring it against a replay set outside the repository.
//!
//! `replay <out_dir> [--last-good] [--min-lines N]` reads one case per line on stdin, tab-separated: the case id,
//! the raw output's path, the exit code (`-` while running), the command and, optionally, the path
//! of the same command's last successful output. Each case gets a fresh home; what the model would
//! be given is written to `<out_dir>/<id>.out`.

use std::io::BufRead;
use std::path::PathBuf;

use ragon_digest::{Digester, Options};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out_dir = PathBuf::from(args.first().expect("usage: replay <out_dir> [--last-good]"));
    let last_good = args.iter().any(|arg| arg == "--last-good");
    let min_lines = args
        .iter()
        .position(|arg| arg == "--min-lines")
        .and_then(|at| args.get(at + 1))
        .map_or(ragon_digest::MIN_LINES, |n| n.parse().expect("--min-lines N"));
    std::fs::create_dir_all(&out_dir).expect("out_dir");
    for line in std::io::stdin().lock().lines() {
        let line = line.expect("stdin");
        let fields: Vec<&str> = line.split('\t').collect();
        let [id, raw, exit, command, rest @ ..] = fields.as_slice() else { continue };
        let home = std::env::temp_dir().join(format!("ragon-digest-replay-{}-{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let digester = Digester::with_options(&home, Options { last_good, min_lines });
        if let Some(good) = rest.first().filter(|path| !path.is_empty()) {
            digester.process(command, std::fs::read(good).expect("good run"), Some(0));
        }
        let exit_code = exit.parse::<i32>().ok();
        let output = digester.process(command, std::fs::read(raw).expect("raw"), exit_code);
        std::fs::write(out_dir.join(format!("{id}.out")), output).expect("write");
        let _ = std::fs::remove_dir_all(&home);
    }
}
