//! The shared vectors (ragon-rs/vectors/log_level_cases.txt): the same lines Python parses.

use ragon_log::LogLevelSpec;

const VECTORS: &str = include_str!("../../vectors/log_level_cases.txt");

#[test]
fn every_vector_parses_as_python_does() {
    let mut checked = 0;
    for line in VECTORS
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
    {
        let fields: Vec<&str> = line.split('|').map(str::trim).collect();
        assert_eq!(fields.len(), 4, "a vector line has four fields: {line}");
        let spec = LogLevelSpec::parse(fields[0].trim_matches(' '));
        let want_default = if fields[1] == "-" {
            None
        } else {
            Some(fields[1].to_string())
        };
        assert_eq!(spec.default, want_default, "default of {line}");
        let want_overrides: Vec<String> = if fields[2] == "-" {
            Vec::new()
        } else {
            fields[2].split(';').map(str::to_string).collect()
        };
        let got_overrides: Vec<String> = spec
            .overrides
            .iter()
            .map(|(component, level)| format!("{component}={level}"))
            .collect();
        assert_eq!(got_overrides, want_overrides, "overrides of {line}");
        assert_eq!(
            spec.warnings.len(),
            fields[3].parse::<usize>().unwrap(),
            "warnings of {line}"
        );
        checked += 1;
    }
    assert!(checked >= 10);
}
