//! Stage A, without a model: which lines of a long output the model sees (answers 11, 24, 32, 44).
//! Every line found by the error search is kept, the last 20 lines always are, warnings are counted
//! with the first few shown, and runs of similar lines are collapsed to a count.

use std::collections::{HashMap, HashSet};
use std::path::Path;

/// The most lines a digest has, its header and the last lines included (answer 24).
pub const MAX_LINES: usize = 60;

/// The last lines a digest always shows (answer 11).
pub const TAIL_LINES: usize = 20;

/// The lines after an error line shown with it while there is room (a panic's assertion and its
/// values, a compiler error's location).
const CONTEXT_AFTER: usize = 3;

/// Distinct warnings shown (answer 32).
const WARNINGS_SHOWN: usize = 3;

/// A shape repeated at least this often is named in the repeat summary (answer 44).
const REPEAT_MIN: usize = 10;

/// The first error lines that get [`Rules::context_before`] lines above them.
const FIRST_ERRORS: usize = 5;

/// Failing test names the header names before "+N more".
const NAMES_SHOWN: usize = 8;

/// The rule steps measured on the digest check set (digest-checkset-1, 2026-11-23), each of which
/// can be switched on alone. [`Rules::TODAY`] is the digest as it was before them; the default is
/// the set that beat plain head and tail truncation there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rules {
    /// Hand the output back when the digest is not shorter than it, in bytes.
    pub shorter_only: bool,
    /// The first lines, always shown like the last ones (the command's banner, a usage line, the
    /// first result).
    pub head: usize,
    /// The header names the error line count and the failing tests.
    pub facts: bool,
    /// Lines shown above each of the first error lines (a traceback's last frame, a compiler's
    /// command line).
    pub context_before: usize,
    /// The first error and its context go first, right under the header; the last lines stay last.
    pub order: bool,
    /// Room left after the errors goes to the first lines, then to the lines before the tail.
    pub fill: bool,
    /// Only the command that produces a pipeline's output decides whether it is a read: a test
    /// run piped to `tail` or `grep` is still digested (answer 29).
    pub producer: bool,
    /// An output of at least this many bytes is digested whatever its line count (0: off).
    pub min_bytes: usize,
    /// A shown line longer than this many characters is cut, its length named (0: off).
    pub max_line_chars: usize,
}

impl Rules {
    /// The digest as it was before the rule steps.
    pub const TODAY: Rules = Rules {
        shorter_only: false,
        head: 0,
        facts: false,
        context_before: 0,
        order: false,
        fill: false,
        producer: false,
        min_bytes: 0,
        max_line_chars: 0,
    };
}

impl Default for Rules {
    /// The steps that beat plain truncation on the check set: 87% of the marked lines kept against
    /// 75% for the first and last 30 lines, 77% for [`Rules::TODAY`].
    fn default() -> Self {
        Rules {
            shorter_only: true,
            head: 3,
            facts: true,
            context_before: 2,
            order: true,
            fill: true,
            producer: true,
            min_bytes: 8000,
            max_line_chars: 400,
        }
    }
}

/// What a line is to the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Error,
    Warning,
    Pass,
    Other,
}

/// Words that mark an error line, in the product's six languages (answer 64): English, German,
/// French, Spanish, Russian and Chinese, plus Japanese and Portuguese for the price of a few words.
const ERROR_WORDS: &[&str] = &[
    "error", "errors", "failed", "failure", "failures", "fail", "fails", "failing", "fatal", "panic", "panicked",
    "exception", "traceback", "abort", "aborted", "aborting", "segfault", "fehler", "fehlgeschlagen", "ausnahme",
    "abgebrochen", "erreur", "erreurs", "échec", "échoué", "échouée", "échoués", "errores", "fallo", "falló",
    "fallido", "fallida", "excepción", "ошибка", "ошибки", "ошибок", "сбой", "исключение", "erro", "falha", "falhou",
];

/// Phrases and marks matched anywhere in the line, lower-cased.
const ERROR_PHRASES: &[&str] = &[
    "##[error]", "timed out", "panicked at", "assertion", "segmentation fault", "core dumped", "undefined reference",
    "не удалось", "错误", "失败", "异常", "エラー", "失敗", "×", "✗", "✘", "❌",
];

/// Counts of nothing that contain an error word: "0 failed", "0 errors".
const ZERO_COUNTS: &[&str] = &["0 failed", "0 errors", "0 error", "0 failures", "0 failure", "0 fail"];

fn words(lower: &str) -> impl Iterator<Item = &str> {
    lower.split(|c: char| !(c.is_alphanumeric() || c == '_')).filter(|word| !word.is_empty())
}

/// Classifies one cleaned line.
pub fn classify(line: &str) -> Class {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Class::Other;
    }
    let lower = trimmed.to_lowercase();
    // Passing test lines, whose names often say "failure" (Rust, pytest, vitest, go).
    if lower.ends_with("... ok")
        || lower.ends_with("... ignored")
        || lower.starts_with("test result: ok")
        || lower.starts_with("--- pass")
        || lower.starts_with("ok ")
        || trimmed.contains('✓')
        || trimmed.contains('✔')
        || trimmed.ends_with("PASSED")
        || trimmed.contains(" PASSED ")
    {
        return Class::Pass;
    }
    if lower.starts_with("warning")
        || lower.starts_with("warn")
        || lower.starts_with("npm warn")
        || lower.starts_with("[warn")
        || lower.starts_with("w:")
        || lower.contains("deprecationwarning")
    {
        return Class::Warning;
    }
    let mut lower = format!(" {lower}");
    for zero in ZERO_COUNTS {
        lower = lower.replace(&format!(" {zero}"), " ");
    }
    if trimmed.contains("FAILED")
        || trimmed.starts_with("FAIL")
        || trimmed.starts_with("E   ")
        || trimmed.starts_with("ERROR")
        || ERROR_PHRASES.iter().any(|phrase| lower.contains(phrase))
        || words(&lower).any(|word| {
            ERROR_WORDS.contains(&word)
                || (word.len() > 5 && (word.ends_with("error") || word.ends_with("exception")))
        })
    {
        return Class::Error;
    }
    Class::Other
}

/// The shape repeats are grouped by: the first word with digits blanked (and the next word when
/// that leaves only digits), plus a test result at the end of the line.
pub fn shape(line: &str) -> String {
    let mut first = String::new();
    for word in line.split_whitespace() {
        let blank: String = word.chars().map(|c| if c.is_ascii_digit() { '#' } else { c }).collect();
        let only_digits = !blank.chars().any(char::is_alphabetic);
        if !first.is_empty() {
            first.push(' ');
        }
        first.push_str(&blank);
        if !only_digits {
            break;
        }
    }
    let last = line.split_whitespace().last().unwrap_or_default();
    match last {
        "ok" | "FAILED" | "ignored" | "passed" | "PASSED" | "skipped" | "SKIPPED" => format!("{first} … {last}"),
        _ => first,
    }
}

/// The status a header names.
fn status(exit_code: Option<i32>) -> String {
    match exit_code {
        Some(code) => format!("exit code {code}"),
        None => "still running".to_string(),
    }
}

/// How to read lines `from..=to` of `path` without re-running the command (answer 49).
pub fn read_hint(path: &Path, from: usize, to: usize) -> String {
    let path = path.display();
    if cfg!(windows) {
        format!("Get-Content '{path}' | Select-Object -Skip {} -First {}", from - 1, to + 1 - from)
    } else {
        format!("sed -n '{from},{to}p' '{path}'")
    }
}

/// The note given instead of a binary output (answer 81).
pub fn binary_note(bytes: usize, exit_code: Option<i32>, path: &Path) -> String {
    format!(
        "[digest] Binary output, {bytes} bytes, {}; not shown. It is saved in {}.\n",
        status(exit_code),
        path.display()
    )
}

/// The digest of `lines` (cleaned, line `i` being line `i + 1` of the raw file at `path`): at most
/// [`MAX_LINES`] lines.
pub fn digest(lines: &[String], exit_code: Option<i32>, path: &Path) -> String {
    digest_with(lines, exit_code, path, &[])
}

/// [`digest`], leaving out the lines the last good run of the same command printed too (`known[i]`
/// for line `i`; empty when there is no such run). It leaves out error lines, which are still named
/// by line number, and the lines after them; the warnings shown and the last lines stay as they are.
pub fn digest_with(lines: &[String], exit_code: Option<i32>, path: &Path, known: &[bool]) -> String {
    let plan = plan(lines, known);
    plan.render(lines, exit_code, path, &plan.extra, None)
}

/// The names of the tests `lines` report failing (pytest, unittest and Django, Rust, Go), in the
/// order they first appear.
pub fn failing_tests(lines: &[String]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for line in lines {
        let trimmed = line.trim();
        let name = if let Some(rest) = trimmed.strip_prefix("FAILED ").or_else(|| trimmed.strip_prefix("ERROR ")) {
            // pytest's short summary: `FAILED path::test_x[param] - AssertionError: ...`.
            rest.split_whitespace().next().filter(|id| id.contains("::")).map(|id| id.rsplit("::").next().unwrap_or(id))
        } else if let Some(id) = trimmed
            .strip_suffix(" FAILED")
            .or_else(|| trimmed.strip_suffix(" ERROR"))
            .or_else(|| trimmed.split_once(" FAILED ").map(|(id, _)| id))
        {
            // pytest -v (`path::test_x FAILED [ 50%]`) and Rust (`test a::b ... FAILED`).
            match id.strip_prefix("test ").and_then(|rest| rest.strip_suffix(" ...")) {
                Some(rust) => Some(rust),
                None => Some(id).filter(|id| id.contains("::") && !id.contains(' ')).map(|id| id.rsplit("::").next().unwrap_or(id)),
            }
        } else if let Some(rest) = trimmed.strip_prefix("FAIL: ").or_else(|| trimmed.strip_prefix("ERROR: ")) {
            // unittest and Django: `FAIL: test_x (module.Class.test_x)`.
            rest.split_whitespace().next().filter(|_| rest.contains(" ("))
        } else if let Some(rest) = trimmed.strip_prefix("--- FAIL: ") {
            rest.split_whitespace().next()
        } else {
            None
        };
        if let Some(name) = name
            && !name.is_empty()
            && !names.iter().any(|known| known == name)
        {
            names.push(name.to_string());
        }
    }
    names
}

/// The header's facts: the error line count, where the first one is, the failing tests.
fn facts(lines: &[String], classes: &[Class]) -> String {
    let errors: Vec<usize> = (0..lines.len()).filter(|index| classes[*index] == Class::Error).collect();
    let mut facts = match errors.first() {
        None => "no error lines".to_string(),
        Some(first) => format!("{} error {}, the first at line {}", errors.len(), plural(errors.len(), "line"), first + 1),
    };
    let names = failing_tests(lines);
    if !names.is_empty() {
        let shown: Vec<&str> = names.iter().take(NAMES_SHOWN).map(String::as_str).collect();
        facts.push_str(&format!("; {} failing {}: {}", names.len(), plural(names.len(), "test"), shown.join(", ")));
        if names.len() > shown.len() {
            facts.push_str(&format!(" (+{} more)", names.len() - shown.len()));
        }
    }
    facts
}

fn plural(count: usize, word: &str) -> String {
    if count == 1 { word.to_string() } else { format!("{word}s") }
}

/// `line`, cut to `max` characters with its length named (0: as it is).
fn cut(line: &str, max: usize) -> String {
    match line.char_indices().nth(max) {
        Some((at, _)) if max > 0 => format!("{} … [line cut, {} characters]", &line[..at], line.chars().count()),
        _ => line.to_string(),
    }
}

/// What stage A chose for one output, before it is written out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// The repeat and warning summary lines.
    pub summary: Vec<String>,
    /// The notes naming error lines not shown.
    pub notes: Vec<String>,
    /// The error lines shown (indices, sorted): in every digest, the model step's too (C7).
    pub mandatory: Vec<usize>,
    /// The other lines shown while there was room (the lines after errors, the first warnings).
    pub extra: Vec<usize>,
    /// The first of the last lines, which are always shown.
    pub tail_start: usize,
    /// The header's facts ([`Rules::facts`]).
    pub facts: Option<String>,
    /// The lines shown first, right under the header ([`Rules::order`]): the first error and the
    /// lines around it.
    pub top: Vec<usize>,
    /// [`Rules::max_line_chars`].
    pub max_line_chars: usize,
}

impl Plan {
    /// Room for lines besides the header, the summary, the notes, the mandatory lines and the tail.
    pub fn room(&self, lines: usize) -> usize {
        (MAX_LINES - 1)
            .saturating_sub(self.summary.len() + self.notes.len() + self.mandatory.len() + (lines - self.tail_start))
    }

    /// The digest showing the mandatory lines, `picked` and the tail; with a retelling, a line for
    /// it and a header that says the model chose lines.
    pub fn render(
        &self,
        lines: &[String],
        exit_code: Option<i32>,
        path: &Path,
        picked: &[usize],
        retelling: Option<&str>,
    ) -> String {
        let total = lines.len();
        let tail_start = self.tail_start;
        let mut selected: Vec<usize> =
            self.mandatory.iter().chain(picked).copied().filter(|index| *index < tail_start).collect();
        selected.sort_unstable();
        selected.dedup();
        let top: Vec<usize> = self.top.iter().copied().filter(|index| selected.binary_search(index).is_ok()).collect();
        let width = total.to_string().len();
        let numbered = |index: usize| format!("{:>width$}: {}", index + 1, cut(&lines[index], self.max_line_chars));
        let (from, to) = match selected.first() {
            Some(first) => (first + 1, (first + 40).min(total)),
            None => (tail_start + 1, total),
        };
        let shown = if retelling.is_some() {
            "lines the local model picked and lines found by an error search"
        } else {
            "lines found by an error search"
        };
        let shown = if top.is_empty() { shown.to_string() } else { format!("the first error with its lines around it, then {shown}") };
        let status = match &self.facts {
            Some(facts) => format!("{}; {facts}", status(exit_code)),
            None => status(exit_code),
        };
        let mut out = vec![format!(
            "[digest] {total} lines, {status}. Shown: {shown}, then the last {} lines, each with its line number; the full output is in {} (read lines with: {}).",
            total - tail_start,
            path.display(),
            read_hint(path, from.max(1), to.max(1)),
        )];
        if let Some(retelling) = retelling {
            out.push(format!("[digest] Retelling by the local model, not verified: {retelling}"));
        }
        out.extend(top.iter().copied().map(numbered));
        out.extend(self.summary.iter().cloned());
        out.extend(self.notes.iter().cloned());
        out.extend(selected.into_iter().filter(|index| !top.contains(index)).map(numbered));
        out.extend((tail_start..total).map(numbered));
        debug_assert!(out.len() <= MAX_LINES);
        let mut text = out.join("\n");
        text.push('\n');
        text
    }
}

/// Stage A's choice for `lines`, leaving out what the last good run printed too (`known`).
pub fn plan(lines: &[String], known: &[bool]) -> Plan {
    plan_with(lines, known, Rules::TODAY)
}

/// [`plan`] under `rules`.
pub fn plan_with(lines: &[String], known: &[bool], rules: Rules) -> Plan {
    let known = |index: usize| known.get(index).copied().unwrap_or(false);
    let total = lines.len();
    let tail_start = total.saturating_sub(TAIL_LINES);
    let classes: Vec<Class> = lines.iter().map(|line| classify(line)).collect();

    // Repeats outside errors, largest first (answer 44).
    let mut groups: HashMap<String, (usize, usize, usize)> = HashMap::new();
    for (index, line) in lines.iter().enumerate() {
        if classes[index] == Class::Error || line.trim().is_empty() {
            continue;
        }
        let entry = groups.entry(shape(line)).or_insert((0, index, index));
        entry.0 += 1;
        entry.2 = index;
    }
    let mut repeats: Vec<(String, (usize, usize, usize))> =
        groups.into_iter().filter(|(_, (count, _, _))| *count >= REPEAT_MIN).collect();
    repeats.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(a.1.1.cmp(&b.1.1)));
    repeats.truncate(3);

    let warnings: Vec<usize> = (0..total).filter(|index| classes[*index] == Class::Warning).collect();

    let mut summary = Vec::new();
    if !repeats.is_empty() {
        let parts: Vec<String> = repeats
            .iter()
            .map(|(shape, (count, first, last))| format!("\"{shape} …\" ×{count} (lines {}-{})", first + 1, last + 1))
            .collect();
        summary.push(format!("[digest] Repeated lines collapsed: {}.", parts.join("; ")));
    }
    if !warnings.is_empty() {
        let first: Vec<String> = warnings.iter().take(10).map(|index| (index + 1).to_string()).collect();
        summary.push(format!(
            "[digest] {} warning lines (at lines {}{}).",
            warnings.len(),
            first.join(", "),
            if warnings.len() > first.len() { ", …" } else { "" }
        ));
    }

    let mut budget = MAX_LINES - 1 - summary.len() - (total - tail_start);
    // The first lines are held back before the errors are counted; error lines among them are
    // counted with the errors.
    let head: Vec<usize> =
        (0..rules.head.min(tail_start)).filter(|index| classes[*index] != Class::Error && !known(*index)).collect();
    budget = budget.saturating_sub(head.len());
    let mut chosen: HashSet<usize> = HashSet::new();
    let mut notes: Vec<String> = Vec::new();

    // Every error line before the tail; when they do not fit, identical ones are named by line
    // number, then the first and the last are shown and the rest named by line number.
    let (seen, errors): (Vec<usize>, Vec<usize>) =
        (0..tail_start).filter(|index| classes[*index] == Class::Error).partition(|index| known(*index));
    if !seen.is_empty() {
        let named: Vec<String> = seen.iter().take(200).map(|index| (index + 1).to_string()).collect();
        notes.push(format!(
            "[digest] {} error lines also printed by the last successful run of this command, not shown: at lines {}{}.",
            seen.len(),
            named.join(", "),
            if seen.len() > named.len() { ", …" } else { "" }
        ));
        budget = budget.saturating_sub(1);
    }
    if errors.len() <= budget {
        chosen.extend(errors.iter().copied());
    } else {
        let mut seen: HashMap<&str, usize> = HashMap::new();
        let mut distinct = Vec::new();
        let mut repeated = Vec::new();
        for index in &errors {
            if seen.insert(lines[*index].trim(), *index).is_none() {
                distinct.push(*index);
            } else {
                repeated.push(*index);
            }
        }
        let mut hidden = repeated;
        let room = budget.saturating_sub(1).max(2);
        if distinct.len() > room {
            let head = room / 2;
            let tail = room - head;
            hidden.extend(distinct[head..distinct.len() - tail].iter().copied());
            distinct = distinct[..head].iter().chain(&distinct[distinct.len() - tail..]).copied().collect();
        }
        hidden.sort_unstable();
        chosen.extend(distinct);
        let named: Vec<String> = hidden.iter().take(200).map(|index| (index + 1).to_string()).collect();
        notes.push(format!(
            "[digest] {} more error lines not shown, at lines {}{}.",
            hidden.len(),
            named.join(", "),
            if hidden.len() > named.len() { ", …" } else { "" }
        ));
        budget = budget.saturating_sub(1);
    }
    budget = budget.saturating_sub(chosen.len());
    let mut mandatory: Vec<usize> = chosen.iter().copied().collect();
    mandatory.sort_unstable();
    chosen.extend(head);

    // The lines above the first errors, nearest first, while there is room.
    for depth in 1..=rules.context_before {
        for anchor in mandatory.iter().take(FIRST_ERRORS) {
            if budget == 0 {
                break;
            }
            let Some(index) = anchor.checked_sub(depth) else { continue };
            if !lines[index].trim().is_empty() && !known(index) && chosen.insert(index) {
                budget -= 1;
            }
        }
    }

    // The lines after each error, nearest first, while there is room.
    let anchors: Vec<usize> = {
        let mut anchors: Vec<usize> = chosen.iter().copied().collect();
        anchors.sort_unstable();
        anchors
    };
    let mut open: Vec<bool> = vec![true; anchors.len()];
    for depth in 1..=CONTEXT_AFTER {
        for (slot, anchor) in anchors.iter().enumerate() {
            if budget == 0 || !open[slot] {
                continue;
            }
            let index = anchor + depth;
            if index >= tail_start || lines[index].trim().is_empty() || classes[index] == Class::Pass {
                open[slot] = false;
                continue;
            }
            if !known(index) && chosen.insert(index) {
                budget -= 1;
            }
        }
    }

    // The first few distinct warnings.
    let mut shown_warnings: HashSet<&str> = HashSet::new();
    for index in warnings.iter().copied().filter(|index| *index < tail_start) {
        if budget == 0 || shown_warnings.len() == WARNINGS_SHOWN {
            break;
        }
        if shown_warnings.insert(lines[index].trim()) && chosen.insert(index) {
            budget -= 1;
        }
    }

    // The first lines, then the lines before the tail, while there is room.
    if rules.fill {
        let free: Vec<usize> = (0..tail_start).filter(|index| !chosen.contains(index) && !known(*index)).collect();
        let take = budget.min(free.len());
        let head = take.div_ceil(2);
        chosen.extend(free[..head].iter().copied());
        chosen.extend(free[free.len() - (take - head)..].iter().copied());
    }

    // The first error and the lines around it, shown first.
    let top: Vec<usize> = match mandatory.first() {
        Some(first) if rules.order => {
            let mut top = vec![*first];
            let mut up = *first;
            while let Some(above) = up.checked_sub(1).filter(|above| chosen.contains(above) && *first - above <= rules.context_before) {
                top.insert(0, above);
                up = above;
            }
            let mut down = *first + 1;
            while down < tail_start && chosen.contains(&down) && down - first <= CONTEXT_AFTER {
                top.push(down);
                down += 1;
            }
            top
        }
        _ => Vec::new(),
    };

    let mut extra: Vec<usize> = chosen.into_iter().filter(|index| mandatory.binary_search(index).is_err()).collect();
    extra.sort_unstable();
    let facts = rules.facts.then(|| facts(lines, &classes));
    Plan { summary, notes, mandatory, extra, tail_start, facts, top, max_line_chars: rules.max_line_chars }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passing_tests_named_after_failures_are_not_errors() {
        assert_eq!(classify("test ledger::tests::the_last_five_failures_are_kept ... ok"), Class::Pass);
        assert_eq!(classify("test result: ok. 776 passed; 0 failed; 1 ignored"), Class::Pass);
        assert_eq!(classify("shell: bash --noprofile --norc -e -o pipefail {0}"), Class::Other);
        assert_eq!(classify("     ✓ acks a refusal as failed 311ms"), Class::Pass);
        assert_eq!(classify("Tests  325 passed | 0 failed"), Class::Other);
    }

    #[test]
    fn errors_are_found_in_every_language_and_format() {
        for line in [
            "error[E0433]: cannot find module or crate `ragon_chat` in this scope",
            "ModuleNotFoundError: No module named 'fcntl'",
            "test x ... FAILED",
            "thread 'x' (9868) panicked at src/a.rs:236:5:",
            "assertion `left == right` failed",
            " FAIL  src/night.test.ts > applies",
            "Error: Test timed out in 5000ms.",
            "E       assert 1 == 2",
            "Fehler: Datei nicht gefunden",
            "Ошибка компиляции",
            "编译错误",
            "java.lang.NullPointerException",
            "##[error]Process completed with exit code 1.",
        ] {
            assert_eq!(classify(line), Class::Error, "{line}");
        }
        assert_eq!(classify("warning: unused import: `std::process::Command`"), Class::Warning);
        assert_eq!(classify("npm warn cleanup [Error: EPERM: operation not permitted"), Class::Warning);
    }

    #[test]
    fn shapes_group_compiler_and_test_runs() {
        assert_eq!(shape("   Compiling serde v1.0.1"), "Compiling");
        assert_eq!(shape("test a::b ... ok"), "test … ok");
        assert_eq!(shape("[12/340] Building CXX"), "[##/###] Building");
    }

    #[test]
    fn many_errors_still_fit_and_are_all_named() {
        let lines: Vec<String> = (0..500).map(|i| format!("error: case {i} broke")).collect();
        let text = digest(&lines, Some(1), Path::new("/tmp/raw.log"));
        assert!(text.lines().count() <= MAX_LINES, "{}", text.lines().count());
        assert!(text.contains("more error lines not shown, at lines"));
        assert!(text.contains("500: error: case 499 broke"));
    }

    #[test]
    fn a_quiet_success_is_its_header_repeats_and_tail() {
        let mut lines: Vec<String> = (0..300).map(|i| format!("   Compiling crate{i} v0.1.0")).collect();
        lines.push("    Finished `release` profile".to_string());
        let text = digest(&lines, Some(0), Path::new("/tmp/raw.log"));
        assert!(text.contains("\"Compiling …\" ×300 (lines 1-300)"), "{text}");
        assert_eq!(text.lines().count(), 1 + 1 + TAIL_LINES);
        assert!(text.ends_with("301:     Finished `release` profile\n"));
    }
}
