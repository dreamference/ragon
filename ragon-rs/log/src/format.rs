//! The line (§5.1) and its typed fields (§5.3), written as the Python writer writes them
//! (`dreamference/config/log_line_formatter.py`). Redaction happens here, before a line exists.

use crate::level::Level;
use crate::redact::{ADDR, Env, Redactor, SECRET, SECRETS_TXT, ip_text, origin, parse_ip, secret_like, urlsplit};
use crate::time::iso_ms;
use hmac::{Hmac, Mac};
use sha2::Sha256;

/// The longest line, in bytes.
pub const MAX_LINE: usize = 4096;
const TRUNCATED: &str = "truncated=1";
/// Marks a byte that was not valid UTF-8 until `quote` writes it as `\x{XX}` (Python keeps such a
/// byte as a lone surrogate; Rust strings cannot, so a private-use character stands in for it).
const BAD_BYTE_BASE: u32 = 0xF700;

/// How a field is written; there is no free-text kind (§5.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Count,
    Bytes,
    Ms,
    S,
    Ratio,
    /// The size of something that is never written: a number, or the byte length of a value.
    SizeOf,
    /// Ragon's own short id (`s-3f2a9c`).
    Id,
    /// One of a fixed set the call site declares; anything else is `other`.
    Enum,
    /// A version, git sha or image digest.
    Version,
    /// A salted pseudonym of a value with many possible forms.
    Pseudo,
    /// The writer's own notes: redacted, quoted when needed.
    Quoted,
    /// Ragon's own paths; the home becomes `~`.
    Path,
    /// Scheme, host and path only.
    Url,
    /// `[addr]` above `debug` unless loopback.
    Addr,
    /// An error class plus its message, written as the field and `<key>_msg`.
    Error,
    /// A kind this writer does not know: written `[type]`.
    Unknown,
}

impl Kind {
    /// The kind a shared fixture names.
    pub fn from_name(name: &str) -> Kind {
        match name {
            "count" => Kind::Count,
            "bytes" => Kind::Bytes,
            "ms" => Kind::Ms,
            "s" => Kind::S,
            "ratio" => Kind::Ratio,
            "size_of" => Kind::SizeOf,
            "id" => Kind::Id,
            "enum" => Kind::Enum,
            "version" => Kind::Version,
            "pseudo" => Kind::Pseudo,
            "quoted" => Kind::Quoted,
            "path" => Kind::Path,
            "url" => Kind::Url,
            "addr" => Kind::Addr,
            "error" => Kind::Error,
            _ => Kind::Unknown,
        }
    }
}

/// The raw value of a field, as the call site has it.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Text(String),
    /// Bytes that may not be UTF-8; an invalid byte is written `\x{XX}`.
    Bytes(Vec<u8>),
    Error { class: String, message: Vec<u8> },
}

macro_rules! from_int {
    ($($t:ty),*) => {$(
        impl From<$t> for Value {
            fn from(n: $t) -> Self {
                i64::try_from(n).map_or(Value::Float(n as f64), Value::Int)
            }
        }
    )*};
}
from_int!(i32, i64, u32, u64, usize);

impl From<f64> for Value {
    fn from(x: f64) -> Self {
        Value::Float(x)
    }
}
impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::Text(s.to_string())
    }
}
impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::Text(s)
    }
}
impl From<Vec<u8>> for Value {
    fn from(b: Vec<u8>) -> Self {
        Value::Bytes(b)
    }
}

/// One typed `key=value` field.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub kind: Kind,
    pub key: String,
    pub value: Value,
    /// The values an `enum` field may take.
    pub allowed: Vec<String>,
}

macro_rules! constructors {
    ($($name:ident => $kind:ident),*) => {$(
        #[doc = concat!("A `", stringify!($name), "` field.")]
        pub fn $name(key: &str, value: impl Into<Value>) -> Field {
            Field::new(Kind::$kind, key, value)
        }
    )*};
}

impl Field {
    /// A field of any kind.
    pub fn new(kind: Kind, key: &str, value: impl Into<Value>) -> Field {
        Field { kind, key: key.to_string(), value: value.into(), allowed: Vec::new() }
    }

    constructors!(
        count => Count, bytes => Bytes, ms => Ms, s => S, ratio => Ratio, size_of => SizeOf, id => Id,
        version => Version, pseudo => Pseudo, quoted => Quoted, path => Path, url => Url, addr => Addr
    );

    /// A value from the fixed set `allowed`; anything else is written `other`.
    pub fn enumeration(key: &str, value: &str, allowed: &[&str]) -> Field {
        Field { allowed: allowed.iter().map(|a| a.to_string()).collect(), ..Field::new(Kind::Enum, key, value) }
    }

    /// An error class and its message, which the writer redacts.
    pub fn error(key: &str, class: &str, message: impl AsRef<[u8]>) -> Field {
        let value = Value::Error { class: class.to_string(), message: message.as_ref().to_vec() };
        Field::new(Kind::Error, key, value)
    }
}

/// Decodes bytes, marking each invalid byte so that `quote` writes it as `\x{XX}`.
fn decode(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut rest = bytes;
    loop {
        match std::str::from_utf8(rest) {
            Ok(s) => {
                out.push_str(s);
                return out;
            }
            Err(e) => {
                let (good, bad) = rest.split_at(e.valid_up_to());
                out.push_str(std::str::from_utf8(good).unwrap_or_default());
                let n = e.error_len().unwrap_or(bad.len());
                for b in &bad[..n] {
                    out.push(char::from_u32(BAD_BYTE_BASE + u32::from(*b)).unwrap_or('?'));
                }
                rest = &bad[n..];
            }
        }
    }
}

/// Python's `str()` of a float, for the cases a log meets.
fn float_text(x: f64) -> String {
    if x.is_finite() && x.fract() == 0.0 && x.abs() < 1e16 { format!("{x:.1}") } else { format!("{x}") }
}

fn text_of(value: &Value) -> String {
    match value {
        Value::Int(n) => n.to_string(),
        Value::Float(x) => float_text(*x),
        Value::Text(s) => s.clone(),
        Value::Bytes(b) => decode(b),
        Value::Error { message, .. } => decode(message),
    }
}

/// Writes a number; anything else is `[number]`.
fn number(value: &Value) -> String {
    match value {
        Value::Int(n) => n.to_string(),
        Value::Float(x) if x.is_finite() => {
            let s = format!("{x:.3}");
            let s = s.trim_end_matches('0').trim_end_matches('.');
            if s.is_empty() { "0".to_string() } else { s.to_string() }
        }
        _ => "[number]".to_string(),
    }
}

/// Quotes and escapes a value: control, C1, bidi and non-UTF-8 bytes never reach the line.
pub fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        let code = c as u32;
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            _ if (BAD_BYTE_BASE + 0x80..BAD_BYTE_BASE + 0x100).contains(&code) => {
                out.push_str(&format!("\\x{{{:02X}}}", code - BAD_BYTE_BASE));
            }
            _ if code < 0x20
                || code == 0x7F
                || (0x80..=0x9F).contains(&code)
                || (0x202A..=0x202E).contains(&code)
                || (0x2066..=0x2069).contains(&code) =>
            {
                out.push_str(&format!("\\u{{{code:04X}}}"));
            }
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

fn bare(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || "._:/~+@[]-".contains(c))
}

/// The value bare when its characters allow it, otherwise quoted.
pub fn render(text: &str) -> String {
    if bare(text) { text.to_string() } else { quote(text) }
}

/// Forces a message into `[a-z0-9][a-z0-9 _.:/-]{0,119}`; a literal from the source already is.
pub fn message(text: &str) -> String {
    let cleaned: String = text
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() || " _.:/-".contains(c) { c } else { '_' })
        .take(120)
        .collect();
    let cleaned = cleaned.trim_matches(' ');
    if cleaned.starts_with(|c: char| c.is_ascii_alphanumeric()) {
        return cleaned.to_string();
    }
    let head: String = cleaned.chars().take(100).collect();
    format!("message {}", head.trim_start_matches(['_', ' ', '.', ':', '/', '-']))
}

/// Forces a key into `[a-z][a-z0-9_]*`.
pub fn key(text: &str) -> String {
    let cleaned: String = text
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' { c } else { '_' })
        .take(64)
        .collect();
    if cleaned.starts_with(|c: char| c.is_ascii_lowercase()) { cleaned } else { format!("k{cleaned}") }
}

fn fits(text: &str, len: (usize, usize), first: fn(char) -> bool, rest: fn(char) -> bool) -> bool {
    let n = text.chars().count();
    let mut chars = text.chars();
    (len.0..=len.1).contains(&n) && chars.next().is_some_and(first) && chars.all(rest)
}

fn is_id(text: &str) -> bool {
    text.split_once('-').is_some_and(|(prefix, rest)| {
        fits(prefix, (1, 4), |c| c.is_ascii_lowercase(), |c| c.is_ascii_lowercase())
            && fits(rest, (2, 32), |c| c.is_ascii_alphanumeric(), |c| c.is_ascii_alphanumeric())
    })
}

fn is_token(text: &str) -> bool {
    let ok = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || "_.-".contains(c);
    fits(text, (1, 64), ok, ok)
}

fn is_version(text: &str) -> bool {
    let ok = |c: char| c.is_ascii_alphanumeric() || "._+:@/-".contains(c);
    fits(text, (1, 100), ok, ok)
}

fn is_class(text: &str) -> bool {
    fits(
        text,
        (1, 81),
        |c| c.is_ascii_alphabetic() || c == '_',
        |c| c.is_ascii_alphanumeric() || c == '_' || c == '.',
    )
}

/// What a writer needs to turn a record into a line.
pub struct Formatter {
    pub redactor: Redactor,
    pub salt: Vec<u8>,
}

impl Formatter {
    /// A formatter with the shared token shapes, a machine's names and a pseudonym salt.
    pub fn new(env: Env, salt: &[u8]) -> Self {
        Formatter { redactor: Redactor::new(SECRETS_TXT, env), salt: salt.to_vec() }
    }

    /// `p-` plus the first 6 bytes of `HMAC-SHA256(salt, kind ‖ 0x1F ‖ value)` in hex (§7.4 rule 2).
    pub fn pseudonym(&self, kind: &str, value: &str) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.salt).expect("hmac takes any key length");
        mac.update(kind.as_bytes());
        mac.update(b"\x1f");
        mac.update(value.as_bytes());
        let digest = mac.finalize().into_bytes();
        format!("p-{}", digest[..6].iter().map(|b| format!("{b:02x}")).collect::<String>())
    }

    /// The `(key, written value)` pairs one field stands for.
    pub fn pairs(&self, field: &Field, level: Level) -> Vec<(String, String)> {
        let key = key(&field.key);
        if secret_like(&key) {
            return vec![(key, SECRET.to_string())];
        }
        let scrub = level <= Level::Info;
        let raw = &field.value;
        let written = match field.kind {
            Kind::Count | Kind::Bytes | Kind::Ms | Kind::S | Kind::Ratio => number(raw),
            Kind::SizeOf => match raw {
                Value::Int(_) | Value::Float(_) => number(raw),
                Value::Bytes(b) => b.len().to_string(),
                other => text_of(other).len().to_string(),
            },
            Kind::Id => Some(text_of(raw)).filter(|t| is_id(t)).unwrap_or_else(|| "[id]".to_string()),
            Kind::Enum => {
                let text = text_of(raw);
                if field.allowed.contains(&text) && is_token(&text) { text } else { "other".to_string() }
            }
            Kind::Version => Some(text_of(raw)).filter(|t| is_version(t)).unwrap_or_else(|| "[version]".to_string()),
            Kind::Pseudo => match raw {
                // Python cannot encode a lone surrogate; its writer catches that as `[error]`.
                Value::Bytes(b) if std::str::from_utf8(b).is_err() => "[error]".to_string(),
                other => self.pseudonym(&key, &text_of(other)),
            },
            Kind::Quoted | Kind::Path => render(&self.redactor.redact(&text_of(raw), scrub)),
            Kind::Url => render(&self.url(&text_of(raw), scrub)),
            Kind::Addr => Self::address(&text_of(raw), level),
            Kind::Error => {
                let (class, message) = match raw {
                    Value::Error { class, message } => (class.as_str(), decode(message)),
                    _ => ("Error", String::new()),
                };
                let class = if is_class(class) { class } else { "Error" };
                let message = quote(&self.redactor.redact(&message, scrub));
                return vec![(key.clone(), class.to_string()), (format!("{key}_msg"), message)];
            }
            Kind::Unknown => "[type]".to_string(),
        };
        vec![(key, written)]
    }

    /// An address: loopback as is, others `[addr]` above `debug` and as is at `debug` (O9).
    fn address(text: &str, level: Level) -> String {
        match parse_ip(text.trim_matches(['[', ']'])) {
            Some((addr, zone)) if addr.is_loopback() || level >= Level::Debug => render(&ip_text(addr, zone)),
            _ => ADDR.to_string(),
        }
    }

    /// Scheme, host and path only; `[url]` when there is no scheme or host.
    fn url(&self, text: &str, scrub: bool) -> String {
        match urlsplit(text) {
            Ok(parts) if !parts.scheme.is_empty() && parts.hostname.is_some() => {
                self.redactor.redact(&format!("{}{}", origin(&parts), parts.path), scrub)
            }
            _ => "[url]".to_string(),
        }
    }

    /// Builds one line without its newline: at most 4 KiB, cut at the last whole field with
    /// `truncated=1`.
    pub fn line(&self, ts_ms: u64, component: &str, level: Level, message_text: &str, fields: &[Field]) -> String {
        let head = format!("{} {component} {} {}", iso_ms(ts_ms), level.word(), message(message_text));
        let mut parts: Vec<String> = fields
            .iter()
            .flat_map(|f| self.pairs(f, level))
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        let joined = |parts: &[String], tail: Option<&str>| {
            let mut all = vec![head.as_str()];
            all.extend(parts.iter().map(String::as_str));
            all.extend(tail);
            all.join(" ")
        };
        let line = joined(&parts, None);
        if line.len() <= MAX_LINE {
            return line;
        }
        while !parts.is_empty() && joined(&parts, Some(TRUNCATED)).len() > MAX_LINE {
            parts.pop();
        }
        joined(&parts, Some(TRUNCATED))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_keys_and_messages_are_written_as_python_writes_them() {
        let n = |x: f64| number(&Value::Float(x));
        assert_eq!((n(1.5), n(2.0), n(-0.0), n(0.0004)), ("1.5".into(), "2".into(), "-0".into(), "0".into()));
        assert_eq!(number(&Value::Float(f64::NAN)), "[number]");
        assert_eq!(key("9Lives"), "k9lives");
        assert_eq!(key("Bad Key!"), "bad_key_");
        assert_eq!(message("Request Done"), "request done");
        assert_eq!(message("__ok"), "message ok");
    }

    #[test]
    fn a_bad_byte_is_written_as_hex() {
        assert_eq!(quote(&decode(b"a\xffb\n")), "\"a\\x{FF}b\\n\"");
    }
}
