//! The fixed workloads (O3): three short prompts and a deterministic long-context prefix, plus the
//! salt line that sets a request's cache state (§3 rule 6). Everything here is deterministic, so two
//! runs send the same bytes and the manifest hash proves it.
use sha2::{Digest, Sha256};

pub const PROSE: &str =
    "Write two short paragraphs explaining why the sky is blue and why sunsets are red.";
pub const CODE: &str = "Write a Python function that merges overlapping intervals in a list of (start, end) pairs, with a docstring and three assert-based tests.";
pub const JSON: &str = "Return only a JSON object describing three fictional books, each with title, author, year and a list of two genres.";

/// The long-context filler: frozen copies of this crate's own sources (`corpus/`, never edited, so the
/// prefix stays the same bytes across versions), repeated with a counter line until it is long
/// enough. The product ships these bytes inside the binary, so every install builds the same prefix.
const SOURCES: [&str; 4] = [
    include_str!("../corpus/figures.txt"),
    include_str!("../corpus/http.txt"),
    include_str!("../corpus/metrics.txt"),
    include_str!("../corpus/reqline.txt"),
];

/// About 32K tokens of code at roughly 3.4 characters a token.
pub const LONG_CHARS: usize = 110_000;

pub fn long_prefix(chars: usize) -> String {
    let mut out = String::with_capacity(chars + 4096);
    let mut part = 0;
    while out.len() < chars {
        out.push_str(&format!("\n// ---- part {part} ----\n"));
        out.push_str(SOURCES[part % SOURCES.len()]);
        part += 1;
    }
    let mut end = chars;
    while !out.is_char_boundary(end) {
        end -= 1;
    }
    out.truncate(end);
    out
}

pub const LONG_QUESTION: &str = "Summarise in two sentences what the code above is for.";
pub const STREAM_QUESTIONS: [&str; 4] = [
    "Name the function in the code above that parses text, and say what it returns.",
    "Which part of the code above deals with percentiles? Answer in one sentence.",
    "List two numbers the code above computes from timings.",
    "What does the code above do with an HTTP response body? Answer in one sentence.",
];

/// The salt line for a cold request: derived from shape, repetition and arm visit, so both arms send
/// the same bytes for the same repetition.
pub fn salt(shape: &str, rep: u32, arm_visit: u32) -> String {
    format!("[profile salt: shape={shape} rep={rep} visit={arm_visit}]\n")
}

pub fn sha256_hex(text: &str) -> String {
    let mut h = Sha256::new();
    h.update(text.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}
