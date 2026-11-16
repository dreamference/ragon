//! Runs every case in `vectors/logging/redact/*.json`, which the Python writer also runs
//! (`tests/test_logging_writer.py`), and compares the line byte for byte; checks every line
//! against `line.re`, and the component and token-shape tables.

use ragon_log::{Env, Field, Formatter, Kind, Level, Redactor, SECRETS_TXT, Value, components};
use regex::Regex;
use serde_json::Value as Json;

/// The shared cases, in the order Python globs them.
const CASES: [(&str, &str); 5] = [
    ("basics", include_str!("../../vectors/logging/redact/basics.json")),
    ("escapes", include_str!("../../vectors/logging/redact/escapes.json")),
    ("rules", include_str!("../../vectors/logging/redact/rules.json")),
    ("tokens", include_str!("../../vectors/logging/redact/tokens.json")),
    ("types", include_str!("../../vectors/logging/redact/types.json")),
];
const LINE_RE: &str = include_str!("../../vectors/logging/line.re");
const COMPONENTS: &str = include_str!("../../vectors/logging/components.txt");
/// The fixed clock of the Python test; the fixtures leave out the timestamp.
const EPOCH_MS: u64 = 1_794_408_000_123;

fn level(word: &str) -> Level {
    Level::from_word(word).flatten().unwrap_or_else(|| panic!("unknown level {word}"))
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

fn value(v: &Json) -> Value {
    match v {
        Json::Number(n) => n.as_i64().map_or_else(|| Value::Float(n.as_f64().unwrap()), Value::Int),
        Json::String(s) => Value::Text(s.clone()),
        other => panic!("no such value in the fixtures: {other}"),
    }
}

fn field(spec: &Json) -> Field {
    let kind = spec["type"].as_str().unwrap();
    let key = spec["key"].as_str().unwrap();
    let v = &spec["value"];
    match kind {
        "error" => {
            let class = v["class"].as_str().unwrap_or("Error");
            let message = match v.get("hex") {
                Some(hex) => unhex(hex.as_str().unwrap()),
                None => v["message"].as_str().unwrap_or_default().as_bytes().to_vec(),
            };
            Field::error(key, class, message)
        }
        "enum" => {
            let allowed: Vec<&str> =
                spec["allowed"].as_array().map(|a| a.iter().filter_map(Json::as_str).collect()).unwrap_or_default();
            Field { allowed: allowed.iter().map(|a| a.to_string()).collect(), ..Field::new(Kind::Enum, key, value(v)) }
        }
        _ => Field::new(Kind::from_name(kind), key, value(v)),
    }
}

#[test]
fn every_shared_case_is_written_byte_for_byte() {
    let line_re = Regex::new(LINE_RE.trim()).unwrap();
    let mut failures = Vec::new();
    let mut total = 0;
    for (file, text) in CASES {
        let cases: Vec<Json> = serde_json::from_str(text).unwrap();
        for case in &cases {
            total += 1;
            let name = format!("{file}/{}", case["name"].as_str().unwrap());
            let env = &case["env"];
            let s = |k: &str| env[k].as_str().unwrap();
            let formatter = Formatter::new(Env::new(s("home"), s("host"), s("user")), s("salt").as_bytes());
            let fields: Vec<Field> = case["fields"].as_array().unwrap().iter().map(field).collect();
            let line = formatter.line(
                EPOCH_MS,
                case["component"].as_str().unwrap(),
                level(case["level"].as_str().unwrap()),
                case["message"].as_str().unwrap(),
                &fields,
            );
            if !line_re.is_match(&line) {
                failures.push(format!("{name}: not in the grammar: {line}"));
            }
            let mut expected = case["expected"].as_str().unwrap().to_string();
            if expected == "PSEUDO" {
                expected = format!("web INFO chat opened chat={}", formatter.pseudonym("chat", "my chat"));
            }
            let got = line.split_once(' ').map_or("", |(_, rest)| rest);
            if got != expected {
                failures.push(format!("{name}:\n  want {expected}\n  got  {got}"));
            }
        }
    }
    assert!(total >= 90, "only {total} cases");
    assert!(failures.is_empty(), "{} of {total} cases differ:\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn the_component_table_is_the_shared_file() {
    let rows: Vec<&str> =
        COMPONENTS.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
    assert_eq!(components(), rows);
    assert!(rows.contains(&"web"));
}

#[test]
fn the_token_shapes_are_the_twelve_python_has() {
    let shapes = Redactor::shapes(SECRETS_TXT);
    assert_eq!(shapes.len(), 12, "{shapes:?}");
    assert_eq!(shapes[0], ("github", "gh", r"\bgh[pousr]_[A-Za-z0-9]{20,}"));
    for (_, _, pattern) in shapes {
        Regex::new(pattern).unwrap();
    }
}
