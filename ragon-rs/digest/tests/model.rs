//! The model step (owner's answer 1 of 2026-11-20): off by default; on, the model's line numbers
//! are copied from the raw output, the error lines and the last lines stay, a wrong format is asked
//! once more and a number out of range leaves stage A's digest.

use std::path::{Path, PathBuf};
use std::pin::pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use ragon_digest::model::{Reply, Selector};
use ragon_digest::{Digester, Options};

fn home(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ragon-digest-model-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Runs a future that never waits (the stand-in answers at once).
fn ready<T>(future: impl Future<Output = T>) -> T {
    let mut future = pin!(future);
    match future.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("the stand-in model never waits"),
    }
}

/// A model that answers the prompts it is given from a list, and records them.
#[derive(Clone)]
struct StandIn {
    replies: Arc<Mutex<Vec<Result<String, String>>>>,
    prompts: Arc<Mutex<Vec<String>>>,
}

impl StandIn {
    fn new(replies: &[Result<&str, &str>]) -> Self {
        let replies = replies.iter().rev().map(|reply| reply.map(str::to_string).map_err(str::to_string)).collect();
        Self { replies: Arc::new(Mutex::new(replies)), prompts: Arc::default() }
    }

    fn asked(&self) -> usize {
        self.prompts.lock().unwrap().len()
    }
}

impl Selector for StandIn {
    fn ask<'a>(&'a self, prompt: &'a str) -> Reply<'a> {
        self.prompts.lock().unwrap().push(prompt.to_string());
        let reply = self.replies.lock().unwrap().pop().unwrap_or(Err("no more replies".to_string()));
        Box::pin(async move { reply })
    }
}

const CAUSE: &str = "error[E0308]: mismatched types in src/ledger.rs";
const NOTE: &str = "note: expected `u64`, found `i64` because of the ledger's total";

/// 400 lines: one error at line 100, a note no error search finds at line 250.
fn output() -> Vec<u8> {
    let mut lines: Vec<String> = (1..=400).map(|n| format!("   Compiling crate-{n} v0.1.0")).collect();
    lines[99] = CAUSE.to_string();
    lines[249] = NOTE.to_string();
    (lines.join("\n") + "\n").into_bytes()
}

fn digester(dir: &Path, model: bool, stand_in: &StandIn) -> Digester {
    Digester::with_options(dir, Options { model, ..Options::default() }).with_selector(Box::new(stand_in.clone()))
}

fn run(digester: &Digester) -> String {
    String::from_utf8(ready(digester.process_async("cargo build", output(), Some(101)))).unwrap()
}

#[test]
fn the_model_step_is_off_by_default() {
    assert!(!Options::default().model);
    let dir = home("off");
    let stand_in = StandIn::new(&[Ok("LINES: 250\nSUMMARY: A type error.")]);
    let digest = run(&digester(&dir, false, &stand_in));
    assert_eq!(stand_in.asked(), 0);
    assert!(!digest.contains(NOTE) && digest.contains(CAUSE));
    let stage_a = Digester::new(&dir, ragon_digest::MIN_LINES).process("cargo build", output(), Some(101));
    assert_eq!(digest.as_bytes(), stage_a.as_slice(), "off, the async path is stage A to the byte");
}

#[test]
fn the_picked_lines_are_copied_from_the_raw_output_beside_the_mandatory_ones() {
    let dir = home("on");
    let stand_in = StandIn::new(&[Ok("<think>the note explains it</think>\nLINES: 250\nSUMMARY: The ledger's total is an i64 where a u64 is expected. Nothing else failed.")]);
    let digest = run(&digester(&dir, true, &stand_in));
    assert_eq!(stand_in.asked(), 1);
    let prompt = stand_in.prompts.lock().unwrap()[0].clone();
    assert!(prompt.contains("Command: cargo build") && prompt.contains(&format!("250: {NOTE}")));
    assert!(digest.contains(&format!("250: {NOTE}")), "{digest}");
    assert!(digest.contains(&format!("100: {CAUSE}")), "the error line stays (C7)");
    assert!(digest.contains("400:    Compiling crate-400 v0.1.0"), "the last lines stay");
    assert!(digest.contains("not verified: The ledger's total is an i64 where a u64 is expected. Nothing else failed."));
    assert!(digest.lines().count() <= ragon_digest::select::MAX_LINES);
    let journal = std::fs::read_to_string(dir.join("digest/journal.jsonl")).unwrap();
    assert!(journal.contains("\"event\":\"model_used\",\"picked\":1"), "{journal}");
}

#[test]
fn a_wrong_format_is_asked_once_more_and_a_number_out_of_range_leaves_stage_a() {
    let stage_a = String::from_utf8(Digester::new(&home("a"), ragon_digest::MIN_LINES).process("cargo build", output(), Some(101))).unwrap();
    let same = |digest: &str| digest.lines().skip(1).eq(stage_a.lines().skip(1));

    let dir = home("retry");
    let stand_in = StandIn::new(&[Ok("The note at line 250 matters."), Ok("LINES: 250\nSUMMARY: A type error.")]);
    let digest = run(&digester(&dir, true, &stand_in));
    assert_eq!(stand_in.asked(), 2);
    assert!(digest.contains(NOTE));

    let dir = home("twice");
    let stand_in = StandIn::new(&[Ok("line 250"), Ok("still line 250"), Ok("LINES: 250\nSUMMARY: x.")]);
    let digest = run(&digester(&dir, true, &stand_in));
    assert_eq!(stand_in.asked(), 2, "one retry, no more");
    assert!(same(&digest), "{digest}");

    let dir = home("range");
    let stand_in = StandIn::new(&[Ok("LINES: 250, 401\nSUMMARY: A type error."), Ok("LINES: 250\nSUMMARY: x.")]);
    let digest = run(&digester(&dir, true, &stand_in));
    assert_eq!(stand_in.asked(), 1, "out of range is discarded, not asked again");
    assert!(same(&digest) && !digest.contains(NOTE));
    let journal = std::fs::read_to_string(dir.join("digest/journal.jsonl")).unwrap();
    assert!(journal.contains("line 401 is out of range"), "{journal}");

    let dir = home("silent");
    let stand_in = StandIn::new(&[Err("timed out after 20 s")]);
    assert!(same(&run(&digester(&dir, true, &stand_in))));
}

#[test]
fn the_call_codex_awaits_can_move_between_threads() {
    fn send<T: Send>(_: &T) {}
    let call = ragon_digest::inside("cargo build", output(), Some(101));
    send(&call);
    let dir = home("send");
    let digester = digester(&dir, true, &StandIn::new(&[]));
    send(&digester.process_async("cargo build", output(), Some(101)));
}
