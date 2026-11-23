//! With the setting on: long outputs become a digest of at most 60 lines naming a raw file whose
//! lines are the output's lines; short, reading and machine-format outputs pass through; three raw
//! reads in a row switch a command kind off and say so in the journal.

use std::path::PathBuf;

use ragon_digest::Digester;
use ragon_digest::select::MAX_LINES;

fn home(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ragon-digest-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn build_log(lines: usize) -> Vec<u8> {
    let mut out = String::new();
    for i in 0..lines {
        if i == lines / 2 {
            out.push_str("\u{1b}[1m\u{1b}[91merror[E0308]\u{1b}[0m: mismatched types\n  --> src/x.rs:4:5\n");
        }
        out.push_str(&format!("   Compiling crate{i} v0.1.0\n"));
    }
    out.into_bytes()
}

fn raw_path(digest: &str) -> PathBuf {
    let start = digest.find("the full output is in ").unwrap() + "the full output is in ".len();
    let rest = &digest[start..];
    PathBuf::from(&rest[..rest.find(" (read lines").unwrap()])
}

#[test]
fn a_long_output_becomes_a_digest_and_its_raw_file_keeps_every_line() {
    let home = home("long");
    let digester = Digester::new(&home, ragon_digest::MIN_LINES);
    let output = build_log(1000);
    let digest = String::from_utf8(digester.process("cargo build", output.clone(), Some(101))).unwrap();
    assert!(digest.lines().count() <= MAX_LINES, "{digest}");
    assert!(digest.starts_with("[digest] 1002 lines, exit code 101; 1 error line, the first at line 501."), "{digest}");
    assert!(digest.contains(" 501: error[E0308]: mismatched types\n 502:   --> src/x.rs:4:5\n"), "{digest}");
    assert!(digest.contains("\"Compiling …\" ×1000"), "{digest}");
    let path = raw_path(&digest);
    assert!(path.starts_with(home.join("digest/raw")));
    let raw = std::fs::read(&path).unwrap();
    assert_eq!(raw, output, "the raw file keeps colours and every byte");
    let line_501 = raw.split(|b| *b == b'\n').nth(500).unwrap();
    assert!(String::from_utf8_lossy(line_501).contains("mismatched types"));
}

#[test]
fn short_reading_and_structured_outputs_pass_through() {
    let home = home("through");
    let digester = Digester::new(&home, ragon_digest::MIN_LINES);
    let short = build_log(150);
    assert_eq!(digester.process("cargo build", short.clone(), Some(0)), short);
    let long = build_log(1000);
    for command in ["cat build.log", "rg -n Compiling", "git log -p"] {
        assert_eq!(digester.process(command, long.clone(), Some(0)), long, "{command}");
    }
    // A build piped to a filter is still a build (answer 29).
    let piped = Digester::new(&home.join("piped"), ragon_digest::MIN_LINES);
    assert!(piped.process("cargo build 2>&1 | tail -n 400", long.clone(), Some(0)).starts_with(b"[digest]"));
    let json: Vec<u8> = format!("[\n{}{{}}\n]\n", "{\"a\": 1},\n".repeat(500)).into_bytes();
    assert_eq!(digester.process("curl -s localhost/api", json.clone(), Some(0)), json);
    let diff: Vec<u8> = format!("diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,500 +1,500 @@\n{}", "-a\n+b\n".repeat(300)).into_bytes();
    assert_eq!(digester.process("hg diff", diff.clone(), Some(0)), diff);
    let csv: Vec<u8> = (0..400).flat_map(|i| format!("{i},a,b,c\n").into_bytes()).collect();
    assert_eq!(digester.process("psql -c x", csv.clone(), Some(0)), csv);
    assert!(!home.join("digest/raw").exists(), "nothing passed through is kept");
}

#[test]
fn a_binary_output_is_a_short_note() {
    let home = home("binary");
    let digester = Digester::new(&home, ragon_digest::MIN_LINES);
    let binary: Vec<u8> = (0..100_000u32).map(|i| (i % 256) as u8).collect();
    let note = String::from_utf8(digester.process("./make-image", binary, Some(0))).unwrap();
    assert!(note.starts_with("[digest] Binary output, 100000 bytes, exit code 0;"), "{note}");
    assert_eq!(note.lines().count(), 1);
}

#[test]
fn three_raw_reads_in_a_row_switch_the_kind_off_for_the_session() {
    let home = home("streak");
    let digester = Digester::new(&home, ragon_digest::MIN_LINES);
    let read = |n: usize, also_read: bool| {
        let output = build_log(300 + n);
        let digest = String::from_utf8(digester.process("cargo test -q", output.clone(), Some(1))).unwrap();
        assert!(digest.starts_with("[digest]"), "digest {n}");
        if also_read {
            let path = raw_path(&digest);
            let command = format!("sed -n '1,50p' '{}'", path.display());
            assert_eq!(digester.process(&command, output.clone(), Some(0)), output);
        }
    };
    read(0, true);
    read(1, true);
    read(2, false); // not read: the streak starts again
    read(3, true);
    read(4, true);
    assert!(!digester.is_off("cargo test"));
    read(5, true);
    assert!(digester.is_off("cargo test"));
    let output = build_log(600);
    assert_eq!(digester.process("cargo test -q", output.clone(), Some(1)), output, "off for the session");
    assert!(digester.process("cargo build", output.clone(), Some(1)).starts_with(b"[digest]"), "other kinds stay on");
    let journal = std::fs::read_to_string(digester.journal_path()).unwrap();
    assert_eq!(journal.matches("\"event\":\"raw_read\"").count(), 5, "{journal}");
    assert!(journal.contains("\"event\":\"switched_off\",\"kind\":\"cargo test\",\"reads\":3"), "{journal}");
}

#[test]
fn old_raw_files_and_those_over_the_cap_are_pruned() {
    let home = home("prune");
    let dir = home.join("digest/raw");
    std::fs::create_dir_all(&dir).unwrap();
    let now = std::time::SystemTime::now();
    for (name, age_hours) in [("a.log", 30u64), ("b.log", 3), ("c.log", 2), ("d.log", 1)] {
        let path = dir.join(name);
        std::fs::write(&path, vec![b'x'; 1000]).unwrap();
        let file = std::fs::File::options().append(true).open(&path).unwrap();
        file.set_modified(now - std::time::Duration::from_secs(age_hours * 3600)).unwrap();
    }
    assert_eq!(ragon_digest::prune(&dir, now, 2500), 2);
    let mut left: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    left.sort();
    assert_eq!(left, ["c.log", "d.log"]);
}
