//! The logging vectors (`ragon-rs/vectors/logging/`, LOGGING §3.3, §5.1): the tests that read the same files as
//! `tests/test_log_vectors.py`, so the line grammar, the components and the redaction cases cannot drift between the
//! languages. `tests/test_log_vectors.py` checks that `CASES` lists every file under `redact/`.

#[cfg(test)]
mod tests {
    use regex_lite::Regex;
    use std::collections::HashSet;

    const COMPONENTS: &str = include_str!("../vectors/logging/components.txt");
    const LINE_RE: &str = include_str!("../vectors/logging/line.re");
    const SECRETS: &str = include_str!("../vectors/logging/secrets.txt");
    const CASES: [&str; 9] = [
        include_str!("../vectors/logging/redact/basics.json"),
        include_str!("../vectors/logging/redact/escapes.json"),
        include_str!("../vectors/logging/redact/rules.json"),
        include_str!("../vectors/logging/redact/tokens.json"),
        include_str!("../vectors/logging/redact/types.json"),
        include_str!("../vectors/logging/redact/field-types.json"),
        include_str!("../vectors/logging/redact/hosts-and-commands.json"),
        include_str!("../vectors/logging/redact/escaping.json"),
        include_str!("../vectors/logging/redact/secret-shapes.json"),
    ];

    fn cases() -> Vec<serde_json::Value> {
        CASES
            .iter()
            .flat_map(|text| serde_json::from_str::<Vec<serde_json::Value>>(text).expect("the cases parse"))
            .collect()
    }

    fn grammar() -> Regex {
        Regex::new(LINE_RE.trim()).expect("line.re compiles")
    }

    #[test]
    fn components_are_valid_and_unique() {
        let names: Vec<&str> = COMPONENTS
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();
        let name = Regex::new("^[a-z][a-z0-9-]*$").unwrap();
        assert!(names.len() >= 30);
        assert_eq!(names.iter().collect::<HashSet<_>>().len(), names.len());
        assert!(names.iter().all(|n| name.is_match(n)));
    }

    #[test]
    fn line_re_accepts_the_grammar_and_refuses_its_breaks() {
        let grammar = grammar();
        let ts = "2026-01-01T00:00:00.000Z";
        assert!(grammar.is_match(&format!("{ts} admin INFO start version=1.2.3 note=\"a \\\"b\\\"\"")));
        assert!(!grammar.is_match(&format!("{ts} Admin INFO start")));
        assert!(!grammar.is_match(&format!("{ts} admin NOTICE start")));
        assert!(!grammar.is_match(&format!("{ts} admin INFO start k=\"a\nb\"")));
    }

    #[test]
    fn every_secret_shape_compiles() {
        let shapes: Vec<&str> = SECRETS.lines().filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
        assert!(shapes.len() >= 10);
        for line in shapes {
            let parts: Vec<&str> = line.split('|').collect();
            assert!(parts.len() >= 3, "{line}");
            Regex::new(&parts[2..].join("|")).unwrap_or_else(|e| panic!("{}: {e}", parts[0]));
        }
    }

    #[test]
    fn every_case_is_well_formed_and_its_line_matches_the_grammar() {
        let grammar = grammar();
        let cases = cases();
        assert!(cases.len() >= 90);
        let mut names = HashSet::new();
        for case in &cases {
            let name = case["name"].as_str().expect("a name");
            assert!(names.insert(name.to_string()), "duplicate case name {name}");
            for key in ["level", "component", "message", "fields", "env", "expected"] {
                assert!(case.get(key).is_some(), "{name}: no {key}");
            }
            let expected = case["expected"].as_str().expect("expected is text");
            if expected != "PSEUDO" {
                let line = format!("2026-01-01T00:00:00.000Z {expected}");
                assert!(grammar.is_match(&line), "{name}: {line}");
            }
        }
    }
}
