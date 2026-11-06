//! `/usage`: token statistics for the current session.
//!
//! Upstream's `/usage` shows ChatGPT plan usage and limit resets, and only for a ChatGPT login, so in
//! Ragon it was hidden and meant nothing. Patch 0011 makes the TUI call [`report`] instead, handing
//! over the counters it already keeps for `/status` as plain numbers, so this crate needs no TUI
//! types and the wording lives here, under test, rather than in a patch.

/// One set of token counters, in the order the TUI's `TokenUsage` declares them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub input: i64,
    pub cached_input: i64,
    pub output: i64,
    pub reasoning_output: i64,
    pub total: i64,
}

impl Counts {
    /// `[input, cached_input, output, reasoning_output, total]`, as the patch passes them.
    pub fn from_array([input, cached_input, output, reasoning_output, total]: [i64; 5]) -> Self {
        Counts {
            input,
            cached_input,
            output,
            reasoning_output,
            total,
        }
    }

    fn cached(&self) -> i64 {
        self.cached_input.max(0)
    }

    fn fresh_input(&self) -> i64 {
        (self.input - self.cached()).max(0)
    }
}

/// The lines `/usage` prints.
///
/// `session` is the running total for this session, `last` the most recent request (whose total is
/// what currently occupies the context window), and `context_window` the served model's
/// `max_model_len` as the launcher wrote it into the catalog.
pub fn report(session: Option<[i64; 5]>, last: Option<[i64; 5]>, context_window: Option<i64>) -> Vec<String> {
    let session = session.map(Counts::from_array).unwrap_or_default();
    if session.total <= 0 && session.input <= 0 && session.output <= 0 {
        return vec![
            "Token usage".to_string(),
            "  No tokens used yet in this session.".to_string(),
        ];
    }

    let mut lines = vec!["Token usage this session".to_string()];
    lines.push(row("Input", session.input, None));
    if session.cached() > 0 {
        lines.push(row(
            "  cached",
            session.cached(),
            Some(format!("{}% of input", percent(session.cached(), session.input))),
        ));
        lines.push(row("  new", session.fresh_input(), None));
    }
    lines.push(row("Output", session.output, None));
    if session.reasoning_output > 0 {
        lines.push(row("  reasoning", session.reasoning_output, None));
    }
    lines.push(row("Total", session.total, None));

    if let Some(last) = last.map(Counts::from_array)
        && last.total > 0
    {
        lines.push(String::new());
        lines.push("Last request".to_string());
        lines.push(row("Input", last.input, None));
        lines.push(row("Output", last.output, None));
        let context = match context_window {
            Some(window) if window > 0 => Some(format!(
                "of {} ({}% used)",
                thousands(window),
                percent(last.total, window)
            )),
            _ => None,
        };
        lines.push(row("Context", last.total, context));
    }
    lines
}

fn row(label: &str, value: i64, note: Option<String>) -> String {
    let base = format!("  {label:<12}{:>12}", thousands(value));
    match note {
        Some(note) => format!("{base}  {note}"),
        None => base,
    }
}

fn percent(part: i64, whole: i64) -> i64 {
    if whole <= 0 {
        return 0;
    }
    ((part.max(0) as f64 / whole as f64) * 100.0).round() as i64
}

/// `1234567` → `1,234,567`.
pub fn thousands(value: i64) -> String {
    let digits = value.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    if value < 0 {
        out.insert(0, '-');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_session_says_so() {
        assert_eq!(report(None, None, Some(32_768))[1], "  No tokens used yet in this session.");
    }

    #[test]
    fn session_totals_split_cached_input_and_show_context_use() {
        let lines = report(
            Some([12_000, 3_000, 2_100, 0, 14_100]),
            Some([9_000, 3_000, 800, 0, 9_800]),
            Some(32_768),
        );
        let text = lines.join("\n");
        assert!(text.contains("Input             12,000"), "{text}");
        assert!(text.contains("    cached           3,000  25% of input"), "{text}");
        assert!(text.contains("    new              9,000"), "{text}");
        assert!(text.contains("Total             14,100"), "{text}");
        assert!(text.contains("Context            9,800  of 32,768 (30% used)"), "{text}");
        assert!(!text.contains("reasoning"), "{text}");
    }

    #[test]
    fn thousands_separators() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(thousands(-12_345), "-12,345");
    }

    #[test]
    fn all_zero_counts_are_an_empty_session() {
        let lines = report(Some([0, 0, 0, 0, 0]), Some([5, 0, 5, 0, 10]), Some(100));
        assert_eq!(lines, ["Token usage", "  No tokens used yet in this session."]);
    }

    #[test]
    fn negative_counts_are_an_empty_session() {
        assert_eq!(report(Some([-5, 0, -1, 0, -6]), None, None).len(), 2);
    }

    #[test]
    fn a_session_without_cached_input_has_no_cached_rows() {
        let text = report(Some([100, 0, 20, 0, 120]), None, None).join("\n");
        assert!(!text.contains("cached") && !text.contains("new"), "{text}");
        assert!(text.starts_with("Token usage this session\n"), "{text}");
    }

    #[test]
    fn reasoning_tokens_get_their_own_row() {
        let text = report(Some([100, 0, 50, 30, 150]), None, None).join("\n");
        assert!(text.contains("  reasoning"), "{text}");
        assert!(text.contains("Total                150"), "{text}");
    }

    #[test]
    fn without_a_last_request_there_is_no_context_block() {
        let text = report(Some([100, 0, 20, 0, 120]), None, Some(1000)).join("\n");
        assert!(!text.contains("Last request") && !text.contains("Context"), "{text}");
        let text = report(Some([100, 0, 20, 0, 120]), Some([0, 0, 0, 0, 0]), Some(1000)).join("\n");
        assert!(!text.contains("Last request"), "{text}");
    }

    #[test]
    fn the_context_row_has_no_note_without_a_usable_window() {
        for window in [None, Some(0), Some(-4)] {
            let lines = report(Some([100, 0, 20, 0, 120]), Some([10, 0, 5, 0, 15]), window);
            let last = lines.last().unwrap();
            assert_eq!(last, "  Context               15", "{window:?}");
        }
    }

    #[test]
    fn cached_input_above_input_never_makes_new_negative() {
        let text = report(Some([100, 150, 20, 0, 120]), None, None).join("\n");
        assert!(text.lines().any(|l| l.starts_with("    new") && l.ends_with("  0") && !l.contains("-")), "{text}");
    }

    #[test]
    fn the_last_request_block_follows_a_blank_line() {
        let lines = report(Some([100, 0, 20, 0, 120]), Some([10, 0, 5, 0, 15]), Some(30));
        let at = lines.iter().position(|l| l == "Last request").unwrap();
        assert_eq!(lines[at - 1], "");
        assert!(lines[at + 3].ends_with("of 30 (50% used)"), "{:?}", lines[at + 3]);
    }

    #[test]
    fn percent_rounds_and_guards_a_zero_whole() {
        assert_eq!(percent(1, 3), 33);
        assert_eq!(percent(2, 3), 67);
        assert_eq!(percent(5, 0), 0);
        assert_eq!(percent(-5, 10), 0);
        assert_eq!(percent(20, 10), 200);
    }

    #[test]
    fn thousands_handles_the_edges_of_the_type() {
        assert_eq!(thousands(-1), "-1");
        assert_eq!(thousands(100_000), "100,000");
        assert_eq!(thousands(i64::MAX), "9,223,372,036,854,775,807");
        assert_eq!(thousands(i64::MIN), "-9,223,372,036,854,775,808");
    }
}
