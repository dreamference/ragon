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
    let mut chosen: HashSet<usize> = HashSet::new();
    let mut notes: Vec<String> = Vec::new();

    // Every error line before the tail; when they do not fit, identical ones are named by line
    // number, then the first and the last are shown and the rest named by line number.
    let errors: Vec<usize> = (0..tail_start).filter(|index| classes[*index] == Class::Error).collect();
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
            if chosen.insert(index) {
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

    let width = total.to_string().len();
    let numbered = |index: usize| format!("{:>width$}: {}", index + 1, lines[index]);
    let mut selected: Vec<usize> = chosen.into_iter().collect();
    selected.sort_unstable();
    let (from, to) = match selected.first() {
        Some(first) => (first + 1, (first + 40).min(total)),
        None => (tail_start + 1, total),
    };
    let mut out = vec![format!(
        "[digest] {total} lines, {}. Shown: lines found by an error search, then the last {} lines, each with its line number; the full output is in {} (read lines with: {}).",
        status(exit_code),
        total - tail_start,
        path.display(),
        read_hint(path, from.max(1), to.max(1)),
    )];
    out.extend(summary);
    out.extend(notes);
    out.extend(selected.into_iter().map(numbered));
    out.extend((tail_start..total).map(numbered));
    debug_assert!(out.len() <= MAX_LINES);
    let mut text = out.join("\n");
    text.push('\n');
    text
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
