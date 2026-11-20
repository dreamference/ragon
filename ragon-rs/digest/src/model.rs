//! The model step (specs/DREAMFERENCE_RAGON_OUTPUT_PREPROCESSING.md §3.3, owner's answer 1 of
//! 2026-11-20): the local model is shown the numbered output and the command, and answers with
//! line NUMBERS and a short retelling. The code copies the text of those lines from the raw output,
//! so the model never writes a line the command did not print. An answer naming a line it was not
//! shown, or any number outside the output, is discarded whole and stage A's digest stands.
//!
//! The crate has no dependencies, so the call itself is the launcher's ([`Selector`]); it bounds
//! the wait. Off by default ([`crate::Options::model`]).

use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::OnceLock;

use crate::select::{self, Class, Plan};

/// The most output lines one prompt shows; a longer output is shown in part (its head, every error
/// line with the lines after it, and its tail), each line keeping its number.
pub const PROMPT_LINES: usize = 1_500;

/// The most characters of output lines one prompt shows (about 16 000 tokens): the node must
/// read the prompt well within the call's 20 seconds, also while it serves others.
pub const PROMPT_CHARS: usize = 40_000;

/// The shares of a shortened prompt's lines and characters that go to the head and to the tail;
/// the error lines get the rest.
const HEAD_SHARE: usize = 15;
const TAIL_SHARE: usize = 40;

/// Lines shown after an error line in a shortened prompt.
const AFTER_ERROR: usize = 2;

/// The most characters of one line in the prompt.
pub const LINE_CHARS: usize = 300;

/// The most characters and sentences of the retelling kept.
pub const RETELLING_CHARS: usize = 400;
const RETELLING_SENTENCES: usize = 3;

/// The reply of a [`Selector`]: the model's text, or why there is none.
pub type Reply<'a> = Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>>;

/// The call to the local model. The launcher's implementation sends the prompt to the node's
/// model with thinking off and gives up after 20 seconds.
pub trait Selector: Send + Sync {
    fn ask<'a>(&'a self, prompt: &'a str) -> Reply<'a>;
}

static SELECTOR: OnceLock<Box<dyn Selector>> = OnceLock::new();

/// Sets this process's model call; the first call wins. Returns whether it was this one.
pub fn set_selector(selector: Box<dyn Selector>) -> bool {
    SELECTOR.set(selector).is_ok()
}

/// The model call in force, if any.
pub fn selector() -> Option<&'static dyn Selector> {
    SELECTOR.get().map(|selector| selector.as_ref())
}

/// What a reply was made of, once checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    /// The picked lines (indices), in the order the reply named them, without repeats.
    pub lines: Vec<usize>,
    /// At most three sentences.
    pub retelling: String,
}

/// Why a reply was not used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Not in the asked format; asked once more.
    Format(String),
    /// A line number outside the output or not shown to the model: the answer is discarded.
    OutOfRange(usize),
}

/// What one line costs in the prompt: its characters up to [`LINE_CHARS`], its number and the
/// separators.
fn cost(lines: &[String], index: usize) -> usize {
    lines[index].chars().take(LINE_CHARS).count() + 10
}

/// A running count of the lines and characters taken against a bound.
struct Budget {
    lines: usize,
    chars: usize,
}

impl Budget {
    fn share(percent: usize) -> Self {
        Self { lines: PROMPT_LINES * percent / 100, chars: PROMPT_CHARS * percent / 100 }
    }

    /// Takes line `index` if it still fits.
    fn take(&mut self, lines: &[String], index: usize) -> bool {
        let cost = cost(lines, index);
        if self.lines == 0 || cost > self.chars {
            return false;
        }
        self.lines -= 1;
        self.chars -= cost;
        true
    }
}

/// The lines (indices) a prompt shows: all of them when they fit in [`PROMPT_LINES`] and
/// [`PROMPT_CHARS`], otherwise the head, the error lines with the lines after them while there is
/// room, and the tail, each within its share.
pub fn shown(lines: &[String]) -> Vec<usize> {
    let total = lines.len();
    if total <= PROMPT_LINES && (0..total).map(|index| cost(lines, index)).sum::<usize>() <= PROMPT_CHARS {
        return (0..total).collect();
    }
    let mut head = Budget::share(HEAD_SHARE);
    let head_end = (0..total).find(|index| !head.take(lines, *index)).unwrap_or(total);
    let mut tail = Budget::share(TAIL_SHARE);
    let tail_start = (head_end..total).rev().find(|index| !tail.take(lines, *index)).map_or(head_end, |index| index + 1);
    let mut room = Budget::share(100 - HEAD_SHARE - TAIL_SHARE);
    let mut middle: Vec<usize> = Vec::new();
    'search: for index in head_end..tail_start {
        if select::classify(&lines[index]) != Class::Error {
            continue;
        }
        for after in index..(index + 1 + AFTER_ERROR).min(tail_start) {
            if middle.last().is_some_and(|last| *last >= after) {
                continue;
            }
            if !room.take(lines, after) {
                break 'search;
            }
            middle.push(after);
        }
    }
    (0..head_end).chain(middle).chain(tail_start..total).collect()
}

fn status(exit_code: Option<i32>) -> String {
    match exit_code {
        Some(code) => format!("it exited with code {code}"),
        None => "it is still running".to_string(),
    }
}

/// The prompt for `command`'s output: the question is the command itself.
pub fn prompt(command: &str, lines: &[String], shown: &[usize], exit_code: Option<i32>) -> String {
    let total = lines.len();
    let width = total.to_string().len();
    let mut out = format!(
        "Below is the output of a shell command an agent ran; {}. Pick the lines the agent must see \
         to act on the result (errors, failed tests, the values in a failed assertion, the final \
         status), then retell the output.\n\
         Answer in exactly this format and nothing else:\n\
         LINES: <line numbers or ranges, for example 12, 40-45; at most 30 lines; or none>\n\
         SUMMARY: <at most three sentences>\n\n\
         Command: {}\n\
         Output, {total} lines{}:\n",
        status(exit_code),
        command.trim(),
        if shown.len() < total { ", only some of them shown" } else { "" },
    );
    let mut previous: Option<usize> = None;
    for index in shown {
        let gap_from = previous.map_or(0, |previous| previous + 1);
        if *index > gap_from {
            out.push_str(&format!("… lines {}-{} not shown\n", gap_from + 1, index));
        }
        let line: String = lines[*index].chars().take(LINE_CHARS).collect();
        out.push_str(&format!("{:>width$}: {line}\n", index + 1));
        previous = Some(*index);
    }
    out
}

/// The reply without a thinking block.
fn without_thinking(reply: &str) -> &str {
    match reply.rfind("</think>") {
        Some(end) => &reply[end + "</think>".len()..],
        None => reply,
    }
}

/// The value after `label:` on a line that starts with it (Markdown emphasis allowed).
fn field<'a>(line: &'a str, label: &str) -> Option<&'a str> {
    let line = line.trim().trim_start_matches(['*', '#', '-', ' ']);
    let head = line.get(..label.len())?;
    if !head.eq_ignore_ascii_case(label) {
        return None;
    }
    let rest = line[label.len()..].trim_start_matches('*').trim_start();
    rest.strip_prefix(':').map(|value| value.trim_start_matches('*').trim())
}

/// The number a token of the LINES field names, tolerating a trailing period.
fn number(token: &str) -> Result<usize, Refusal> {
    token.trim().trim_end_matches('.').parse::<usize>().map_err(|_| Refusal::Format(format!("not a line number: {token}")))
}

/// Checks a reply against the output's `total` lines and the `shown` ones (sorted indices).
pub fn parse(reply: &str, total: usize, shown: &[usize]) -> Result<Answer, Refusal> {
    let reply = without_thinking(reply);
    let mut numbers: Option<&str> = None;
    let mut summary: Option<String> = None;
    for line in reply.lines() {
        if summary.is_none()
            && let Some(value) = field(line, "LINES")
        {
            numbers.get_or_insert(value);
        } else if let Some(value) = field(line, "SUMMARY") {
            summary.get_or_insert_with(String::new).push_str(value);
        } else if let Some(summary) = summary.as_mut()
            && !line.trim().is_empty()
        {
            summary.push(' ');
            summary.push_str(line.trim());
        }
    }
    let numbers = numbers.ok_or_else(|| Refusal::Format("no LINES field".to_string()))?;
    let summary = summary.ok_or_else(|| Refusal::Format("no SUMMARY field".to_string()))?;
    let mut picked: Vec<usize> = Vec::new();
    let named = numbers.trim().trim_matches(['.', '[', ']']);
    if !(named.is_empty() || named.eq_ignore_ascii_case("none")) {
        for token in named.split(|c: char| c == ',' || c == ';' || c.is_whitespace()).filter(|token| !token.is_empty()) {
            let (from, to) = match token.split_once(['-', '–']) {
                Some((from, to)) => (number(from)?, number(to)?),
                None => {
                    let line = number(token)?;
                    (line, line)
                }
            };
            if from > to {
                return Err(Refusal::Format(format!("a range runs backwards: {token}")));
            }
            for line in from..=to {
                if line == 0 || line > total || shown.binary_search(&(line - 1)).is_err() {
                    return Err(Refusal::OutOfRange(line));
                }
                if !picked.contains(&(line - 1)) {
                    picked.push(line - 1);
                }
            }
        }
    }
    Ok(Answer { lines: picked, retelling: retelling(&summary) })
}

/// The first three sentences of `summary`, on one line, within [`RETELLING_CHARS`].
pub fn retelling(summary: &str) -> String {
    let flat = summary.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut end = flat.len();
    let mut sentences = 0;
    for (at, c) in flat.char_indices() {
        if matches!(c, '.' | '!' | '?' | '。') && flat[at + c.len_utf8()..].chars().next().is_none_or(char::is_whitespace) {
            sentences += 1;
            if sentences == RETELLING_SENTENCES {
                end = at + c.len_utf8();
                break;
            }
        }
    }
    let kept = &flat[..end];
    if kept.chars().count() <= RETELLING_CHARS {
        return kept.to_string();
    }
    let cut: String = kept.chars().take(RETELLING_CHARS - 1).collect();
    format!("{cut}…")
}

/// The digest with the model's picks: the mandatory lines and the tail stay (C7), the picks fill
/// the room first and stage A's other lines the rest.
pub fn compose(plan: &Plan, lines: &[String], exit_code: Option<i32>, path: &Path, answer: &Answer) -> String {
    let room = plan.room(lines.len()).saturating_sub(1);
    let mut picked: Vec<usize> = Vec::new();
    for index in answer.lines.iter().chain(&plan.extra).copied() {
        if picked.len() == room {
            break;
        }
        if index < plan.tail_start && plan.mandatory.binary_search(&index).is_err() && !picked.contains(&index) {
            picked.push(index);
        }
    }
    let retelling = if answer.retelling.is_empty() { "(none given)" } else { answer.retelling.as_str() };
    plan.render(lines, exit_code, path, &picked, Some(retelling))
}

/// How one model step ended, for the journal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The answer was used.
    Used { picked: usize },
    /// No usable answer; stage A's digest stands.
    Discarded(String),
}

/// Asks `selector` about `lines` and returns the digest built from its answer, or why stage A
/// stands. A reply in the wrong format is asked once more; a number out of range is not.
pub async fn step(
    selector: &dyn Selector,
    command: &str,
    plan: &Plan,
    lines: &[String],
    exit_code: Option<i32>,
    path: &Path,
) -> (Option<String>, Outcome) {
    let shown = shown(lines);
    let prompt = prompt(command, lines, &shown, exit_code);
    let mut last = String::new();
    for _ in 0..2 {
        let reply = match selector.ask(&prompt).await {
            Ok(reply) => reply,
            Err(why) => return (None, Outcome::Discarded(format!("no reply: {why}"))),
        };
        match parse(&reply, lines.len(), &shown) {
            Ok(answer) => {
                let text = compose(plan, lines, exit_code, path, &answer);
                return (Some(text), Outcome::Used { picked: answer.lines.len() });
            }
            Err(Refusal::OutOfRange(line)) => {
                return (None, Outcome::Discarded(format!("line {line} is out of range")));
            }
            Err(Refusal::Format(why)) => last = why,
        }
    }
    (None, Outcome::Discarded(format!("wrong format twice: {last}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbered(total: usize) -> Vec<String> {
        (1..=total).map(|n| format!("line {n}")).collect()
    }

    #[test]
    fn numbers_and_ranges_are_read_and_the_retelling_is_cut_to_three_sentences() {
        let shown: Vec<usize> = (0..100).collect();
        let reply = "<think>maybe 7</think>\n**LINES:** 12, 40-42; 12 none\n**SUMMARY:** One. Two!\nThree? Four.";
        assert_eq!(
            parse(reply, 100, &shown),
            Err(Refusal::Format("not a line number: none".to_string())),
            "a stray word is a wrong format"
        );
        let reply = "<think>maybe 7</think>\n**LINES:** 12, 40-42; 12\n**SUMMARY:** One. Two!\nThree? Four.";
        let answer = parse(reply, 100, &shown).unwrap();
        assert_eq!(answer.lines, vec![11, 39, 40, 41]);
        assert_eq!(answer.retelling, "One. Two! Three?");
        let none = parse("LINES: none\nSUMMARY: It built.", 100, &shown).unwrap();
        assert!(none.lines.is_empty());
        assert_eq!(none.retelling, "It built.");
    }

    #[test]
    fn a_number_outside_the_output_or_the_shown_lines_discards_the_answer() {
        let shown: Vec<usize> = (0..100).collect();
        assert_eq!(parse("LINES: 3, 101\nSUMMARY: x.", 100, &shown), Err(Refusal::OutOfRange(101)));
        assert_eq!(parse("LINES: 0\nSUMMARY: x.", 100, &shown), Err(Refusal::OutOfRange(0)));
        assert_eq!(parse("LINES: 98-102\nSUMMARY: x.", 100, &shown), Err(Refusal::OutOfRange(101)));
        let partial: Vec<usize> = (0..10).chain(90..100).collect();
        assert_eq!(parse("LINES: 50\nSUMMARY: x.", 100, &partial), Err(Refusal::OutOfRange(50)));
        assert!(matches!(parse("SUMMARY: x.", 100, &shown), Err(Refusal::Format(_))));
        assert!(matches!(parse("LINES: 9-3\nSUMMARY: x.", 100, &shown), Err(Refusal::Format(_))));
    }

    #[test]
    fn a_long_output_is_shown_in_part_with_its_numbers_and_the_gaps_named() {
        let mut lines = numbered(5_000);
        lines[2_999] = "error: the build failed".to_string();
        let shown = shown(&lines);
        assert!(shown.len() <= PROMPT_LINES);
        assert!(shown.contains(&2_999) && shown.contains(&3_000) && shown.contains(&4_999) && shown.contains(&0));
        let prompt = prompt("cargo build", &lines, &shown, Some(101));
        assert!(prompt.contains("Command: cargo build"));
        assert!(prompt.contains("3000: error: the build failed"));
        assert!(prompt.contains("… lines 226-2999 not shown"), "{prompt}");
        assert!(prompt.contains("only some of them shown"));
        assert!(prompt.contains("exited with code 101"));
    }

    #[test]
    fn long_lines_keep_the_prompt_within_its_characters() {
        let mut lines: Vec<String> = (0..1_200).map(|n| format!("{n} {}", "x".repeat(400))).collect();
        lines[700] = "error: the build failed".to_string();
        let shown = shown(&lines);
        assert!(shown.contains(&0) && shown.contains(&700) && shown.contains(&1_199));
        let prompt = prompt("cargo build", &lines, &shown, Some(101));
        assert!(prompt.chars().count() < PROMPT_CHARS + 2_000, "{}", prompt.chars().count());
        let short = numbered(1_000);
        assert_eq!(super::shown(&short).len(), 1_000, "a short output is shown whole");
    }
}
