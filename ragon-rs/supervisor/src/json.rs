//! A small JSON reader and writer for job requests and `record.json` (the same files the Linux
//! node's `node_job.py` reads and writes). The crate takes no dependency, so a change here never
//! touches a lock file; the values are a few dozen keys, so speed does not matter.
//!
//! Objects keep their key order (a `Vec` of pairs), numbers are `f64` (job sizes stay far below
//! 2^53), and the writer escapes control characters and leaves other characters as they are.

use std::fmt::Write as _;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Sets `key` (replacing its value in place, or appending it). A no-op on a non-object.
    pub fn set(&mut self, key: &str, value: Value) {
        if let Value::Object(pairs) = self {
            match pairs.iter_mut().find(|(k, _)| k == key) {
                Some(pair) => pair.1 = value,
                None => pairs.push((key.to_string(), value)),
            }
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        self.as_f64().filter(|n| *n >= 0.0 && n.fract() == 0.0 && *n < 1.8e19).map(|n| n as u64)
    }

    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Python's truthiness, which `node_job.py` uses for optional fields.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Number(n) => *n != 0.0,
            Value::String(s) => !s.is_empty(),
            Value::Array(items) => !items.is_empty(),
            Value::Object(pairs) => !pairs.is_empty(),
        }
    }

    pub fn str(s: impl Into<String>) -> Value {
        Value::String(s.into())
    }

    pub fn opt_str(s: Option<impl Into<String>>) -> Value {
        s.map(|s| Value::String(s.into())).unwrap_or(Value::Null)
    }

    pub fn num(n: impl Into<f64>) -> Value {
        Value::Number(n.into())
    }

    /// Compact text, as `json.dumps(..., separators)` would not matter: any JSON reader takes it.
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        write_value(self, &mut out, None, 0);
        out
    }

    /// Indented text (two spaces), for files people read.
    pub fn to_json_pretty(&self) -> String {
        let mut out = String::new();
        write_value(self, &mut out, Some(2), 0);
        out.push('\n');
        out
    }
}

fn write_value(value: &Value, out: &mut String, indent: Option<usize>, depth: usize) {
    let newline = |out: &mut String, depth: usize| {
        if let Some(step) = indent {
            out.push('\n');
            out.extend(std::iter::repeat_n(' ', step * depth));
        }
    };
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e17 {
                let _ = write!(out, "{}", *n as i64);
            } else if n.is_finite() {
                let _ = write!(out, "{n}");
            } else {
                out.push_str("null");
            }
        }
        Value::String(s) => write_string(s, out),
        Value::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                    if indent.is_none() {
                        out.push(' ');
                    }
                }
                newline(out, depth + 1);
                write_value(item, out, indent, depth + 1);
            }
            newline(out, depth);
            out.push(']');
        }
        Value::Object(pairs) => {
            if pairs.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (i, (key, item)) in pairs.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                    if indent.is_none() {
                        out.push(' ');
                    }
                }
                newline(out, depth + 1);
                write_string(key, out);
                out.push_str(": ");
                write_value(item, out, indent, depth + 1);
            }
            newline(out, depth);
            out.push('}');
        }
    }
}

fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Parses one JSON value (surrounding whitespace allowed, nothing else after it).
pub fn parse(text: &str) -> Result<Value, String> {
    let mut p = Parser { bytes: text.as_bytes(), pos: 0, depth: 0 };
    p.skip_ws();
    let value = p.value()?;
    p.skip_ws();
    if p.pos != p.bytes.len() {
        return Err(format!("unexpected text after the JSON value at byte {}", p.pos));
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    depth: usize,
}

const MAX_DEPTH: usize = 64;

impl Parser<'_> {
    fn err<T>(&self, what: &str) -> Result<T, String> {
        Err(format!("{what} at byte {}", self.pos))
    }

    fn skip_ws(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.bytes.get(self.pos) {
            self.pos += 1;
        }
    }

    fn eat(&mut self, literal: &str) -> bool {
        if self.bytes[self.pos..].starts_with(literal.as_bytes()) {
            self.pos += literal.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> Result<Value, String> {
        match self.bytes.get(self.pos) {
            None => self.err("unexpected end of JSON"),
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string().map(Value::String),
            Some(b't') if self.eat("true") => Ok(Value::Bool(true)),
            Some(b'f') if self.eat("false") => Ok(Value::Bool(false)),
            Some(b'n') if self.eat("null") => Ok(Value::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => self.err("unexpected character"),
        }
    }

    fn nested(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > MAX_DEPTH { self.err("JSON nested too deeply") } else { Ok(()) }
    }

    fn object(&mut self) -> Result<Value, String> {
        self.nested()?;
        self.pos += 1;
        let mut pairs: Vec<(String, Value)> = Vec::new();
        self.skip_ws();
        if self.eat("}") {
            self.depth -= 1;
            return Ok(Value::Object(pairs));
        }
        loop {
            self.skip_ws();
            if self.bytes.get(self.pos) != Some(&b'"') {
                return self.err("expected a key");
            }
            let key = self.string()?;
            self.skip_ws();
            if !self.eat(":") {
                return self.err("expected ':'");
            }
            self.skip_ws();
            let value = self.value()?;
            // A repeated key keeps the last value, as Python's json does.
            match pairs.iter_mut().find(|(k, _)| *k == key) {
                Some(pair) => pair.1 = value,
                None => pairs.push((key, value)),
            }
            self.skip_ws();
            if self.eat(",") {
                continue;
            }
            if self.eat("}") {
                self.depth -= 1;
                return Ok(Value::Object(pairs));
            }
            return self.err("expected ',' or '}'");
        }
    }

    fn array(&mut self) -> Result<Value, String> {
        self.nested()?;
        self.pos += 1;
        let mut items = Vec::new();
        self.skip_ws();
        if self.eat("]") {
            self.depth -= 1;
            return Ok(Value::Array(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value()?);
            self.skip_ws();
            if self.eat(",") {
                continue;
            }
            if self.eat("]") {
                self.depth -= 1;
                return Ok(Value::Array(items));
            }
            return self.err("expected ',' or ']'");
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self.bytes.get(self.pos..self.pos + 4).ok_or("unexpected end in \\u escape")?;
        let text = std::str::from_utf8(digits).map_err(|_| "bad \\u escape".to_string())?;
        let code = u32::from_str_radix(text, 16).map_err(|_| format!("bad \\u escape at byte {}", self.pos))?;
        self.pos += 4;
        Ok(code)
    }

    fn string(&mut self) -> Result<String, String> {
        self.pos += 1;
        let mut out = String::new();
        loop {
            let start = self.pos;
            while let Some(&b) = self.bytes.get(self.pos) {
                if b == b'"' || b == b'\\' || b < 0x20 {
                    break;
                }
                self.pos += 1;
            }
            // The input is a &str and we stopped on ASCII, so the slice is valid UTF-8.
            out.push_str(std::str::from_utf8(&self.bytes[start..self.pos]).map_err(|e| e.to_string())?);
            match self.bytes.get(self.pos) {
                None => return self.err("unterminated string"),
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.pos += 1;
                    let escape = *self.bytes.get(self.pos).ok_or("unterminated escape")?;
                    self.pos += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let first = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&first) && self.eat("\\u") {
                                let second = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&second) {
                                    return self.err("bad surrogate pair");
                                }
                                0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
                            } else {
                                first
                            };
                            // A lone surrogate becomes U+FFFD (Rust strings cannot hold it).
                            out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                        }
                        _ => return self.err("bad escape"),
                    }
                }
                Some(_) => return self.err("control character in string"),
            }
        }
    }

    fn number(&mut self) -> Result<Value, String> {
        let start = self.pos;
        if self.bytes.get(self.pos) == Some(&b'-') {
            self.pos += 1;
        }
        while let Some(b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-') = self.bytes.get(self.pos) {
            self.pos += 1;
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos]).map_err(|e| e.to_string())?;
        text.parse::<f64>().map(Value::Number).map_err(|_| format!("bad number {text:?} at byte {start}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_record() {
        let text = r#"{"id": "20261125-1130-abc", "command": ["sh", "-c", "echo \"hi\"\n"], "memory_bytes": 1073741824,
            "test": null, "ok": true, "nested": {"a": [1, 2.5, -3]}, "author": {"name": "Ré 😀"}}"#;
        let value = parse(text).unwrap();
        assert_eq!(value.get("id").and_then(Value::as_str), Some("20261125-1130-abc"));
        assert_eq!(value.get("memory_bytes").and_then(Value::as_u64), Some(1 << 30));
        assert_eq!(value.get("command").unwrap().as_array().unwrap()[2].as_str(), Some("echo \"hi\"\n"));
        assert_eq!(value.get("author").unwrap().get("name").unwrap().as_str(), Some("Ré 😀"));
        assert!(value.get("test").unwrap().is_null());
        assert_eq!(parse(&value.to_json()).unwrap(), value);
        assert_eq!(parse(&value.to_json_pretty()).unwrap(), value);
    }

    #[test]
    fn refuses_broken_text() {
        for bad in ["", "{", "{\"a\" 1}", "[1,]", "\"abc", "{} x", "tru", "\"a\u{1}\"", "[1 2]"] {
            assert!(parse(bad).is_err(), "{bad:?} parsed");
        }
        let deep = "[".repeat(100) + &"]".repeat(100);
        assert!(parse(&deep).is_err());
    }

    #[test]
    fn set_replaces_in_place_and_appends() {
        let mut v = parse(r#"{"a": 1, "b": 2}"#).unwrap();
        v.set("a", Value::str("x"));
        v.set("c", Value::Null);
        assert_eq!(v.to_json(), r#"{"a": "x", "b": 2, "c": null}"#);
        assert!(!Value::str("").truthy() && Value::num(1).truthy() && !Value::Array(vec![]).truthy());
    }
}
