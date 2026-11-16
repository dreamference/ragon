//! `redact()` (§7.4 rule 3) and the secret-like key rule (rule 4), rule for rule the Python writer's
//! (`dreamference/config/log_redactor.py`); the shared fixtures in `vectors/logging/redact/` hold
//! the two to the same bytes.
//!
//! Token shapes are data: `vectors/logging/secrets.txt`, read by every language's writer. The
//! `regex` crate has no look-around; [`sub_where`] checks the character before and after a match
//! instead, which gives the same matches here because no rule with a look-around has a shorter
//! alternative that could pass where the longest failed.

use regex::{Captures, Regex};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// The shared token shapes, one per line: `name|keyword|pattern`.
pub const SECRETS_TXT: &str = include_str!("../../vectors/logging/secrets.txt");
/// What a secret becomes.
pub const SECRET: &str = "[secret]";
/// What an address becomes.
pub const ADDR: &str = "[addr]";

const SECRET_SUFFIXES: [&str; 8] = ["token", "key", "secret", "password", "passwd", "cookie", "credential", "auth"];

/// What a machine's own names are, read once at start-up so no list in the repository names anyone.
#[derive(Clone, Debug, Default)]
pub struct Env {
    pub home: String,
    pub host: String,
    pub user: String,
}

impl Env {
    /// The names, as the Python writer holds them (the home without a trailing separator).
    pub fn new(home: &str, host: &str, user: &str) -> Self {
        Env { home: home.trim_end_matches(['/', '\\']).to_string(), host: host.to_string(), user: user.to_string() }
    }

    /// The real machine's names: the kernel's host name, the login name and the home.
    pub fn from_system() -> Self {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let host = ["/proc/sys/kernel/hostname", "/etc/hostname"]
            .iter()
            .find_map(|p| std::fs::read_to_string(p).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()))
            .or_else(|| var("HOSTNAME"))
            .unwrap_or_default();
        let user = ["LOGNAME", "USER", "LNAME", "USERNAME"].iter().find_map(|k| var(k)).unwrap_or_default();
        let home = var("HOME").or_else(|| var("USERPROFILE")).unwrap_or_default();
        Env::new(&home, &host, &user)
    }
}

/// True when a field or flag name is secret-like: its last `_` or `-` segment is on the list.
pub fn secret_like(key: &str) -> bool {
    key.to_lowercase().rsplit(['_', '-']).next().is_some_and(|last| SECRET_SUFFIXES.contains(&last))
}

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("a fixed pattern compiles")
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn before(text: &str, at: usize) -> Option<char> {
    text[..at].chars().next_back()
}

fn after(text: &str, at: usize) -> Option<char> {
    text[at..].chars().next()
}

/// `re.sub` with a look-behind and a look-ahead: `keep(start, end)` says whether the match at
/// `start..end` stands; a match that does not is retried one character further on.
fn sub_where(
    pattern: &Regex,
    text: &str,
    keep: impl Fn(usize, usize) -> bool,
    replace: impl Fn(&Captures) -> String,
) -> String {
    let mut out = String::with_capacity(text.len());
    let (mut last, mut pos) = (0, 0);
    while pos <= text.len() {
        let Some(caps) = pattern.captures_at(text, pos) else { break };
        let m = caps.get(0).expect("group 0 is the match");
        if !keep(m.start(), m.end()) {
            pos = m.start() + after(text, m.start()).map_or(1, char::len_utf8);
            continue;
        }
        out.push_str(&text[last..m.start()]);
        out.push_str(&replace(&caps));
        last = m.end();
        pos = if m.end() > m.start() { m.end() } else { m.end() + after(text, m.end()).map_or(1, char::len_utf8) };
    }
    out.push_str(&text[last..]);
    out
}

/// A literal name replaced where the characters around it allow (`re.sub(lookbehind + name + lookahead)`).
fn sub_name(
    pattern: &Regex,
    text: &str,
    bad_before: impl Fn(char) -> bool,
    bad_after: impl Fn(char) -> bool,
    with: &str,
) -> String {
    sub_where(
        pattern,
        text,
        |s, e| !before(text, s).is_some_and(&bad_before) && !after(text, e).is_some_and(&bad_after),
        |_| with.to_string(),
    )
}

/// What `urllib.parse.urlsplit` gives, for the parts a log keeps.
#[derive(Debug, PartialEq)]
pub(crate) struct Split {
    pub scheme: String,
    pub path: String,
    pub hostname: Option<String>,
    pub port: Option<u32>,
}

/// An address as `ipaddress.ip_address` reads text: an IPv6 address may carry a `%zone`.
pub(crate) fn parse_ip(text: &str) -> Option<(IpAddr, Option<&str>)> {
    if let Ok(v4) = text.parse::<Ipv4Addr>() {
        return Some((IpAddr::V4(v4), None));
    }
    let (addr, zone) = match text.split_once('%') {
        Some((_, zone)) if zone.is_empty() || zone.contains('%') => return None,
        Some((addr, zone)) => (addr, Some(zone)),
        None => (text, None),
    };
    addr.parse::<Ipv6Addr>().ok().map(|a| (IpAddr::V6(a), zone))
}

/// `str()` of an address: IPv6 compressed as Python writes it (the first longest run of two or more
/// zero groups becomes `::`, an IPv4-mapped address stays in hex).
pub(crate) fn ip_text(addr: IpAddr, zone: Option<&str>) -> String {
    let IpAddr::V6(v6) = addr else { return addr.to_string() };
    let groups = v6.segments();
    let (mut best, mut best_len, mut run, mut run_len) = (0, 0, 0, 0);
    for (i, g) in groups.iter().enumerate() {
        if *g == 0 {
            if run_len == 0 {
                run = i;
            }
            run_len += 1;
            if run_len > best_len {
                (best, best_len) = (run, run_len);
            }
        } else {
            run_len = 0;
        }
    }
    let hex = |gs: &[u16]| gs.iter().map(|g| format!("{g:x}")).collect::<Vec<_>>().join(":");
    let mut out = if best_len > 1 {
        format!("{}::{}", hex(&groups[..best]), hex(&groups[best + best_len..]))
    } else {
        hex(&groups)
    };
    if let Some(zone) = zone {
        out.push('%');
        out.push_str(zone);
    }
    out
}

fn check_bracketed_host(hostname: &str) -> Result<(), ()> {
    if hostname.starts_with('v') {
        let future = hostname[1..]
            .split_once('.')
            .is_some_and(|(hex, rest)| !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit()) && !rest.is_empty());
        return if future { Ok(()) } else { Err(()) };
    }
    match parse_ip(hostname) {
        Some((IpAddr::V6(_), _)) => Ok(()),
        _ => Err(()),
    }
}

fn check_bracketed_netloc(netloc: &str) -> Result<(), ()> {
    let hostinfo = netloc.rsplit_once('@').map_or(netloc, |(_, h)| h);
    let hostname = match hostinfo.split_once('[') {
        Some((before, _)) if !before.is_empty() => return Err(()),
        Some((_, bracketed)) => {
            let (hostname, port) = bracketed.split_once(']').unwrap_or((bracketed, ""));
            if !port.is_empty() && !port.starts_with(':') {
                return Err(());
            }
            hostname
        }
        None => hostinfo.split_once(':').map_or(hostinfo, |(h, _)| h),
    };
    check_bracketed_host(hostname)
}

/// `urlsplit(text)` and its `hostname` and `port`, as Python 3.12 computes them; `Err` where Python
/// raises `ValueError`. The NFKC check of a non-ASCII network location is not made.
pub(crate) fn urlsplit(text: &str) -> Result<Split, ()> {
    let url: String =
        text.trim_start_matches(|c: char| c <= ' ').chars().filter(|c| !matches!(c, '\t' | '\r' | '\n')).collect();
    let mut scheme = String::new();
    let mut rest = url.as_str();
    if let Some(i) = url.find(':')
        && i > 0
        && url.starts_with(|c: char| c.is_ascii_alphabetic())
        && url[..i].chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
    {
        scheme = url[..i].to_ascii_lowercase();
        rest = &url[i + 1..];
    }
    let mut netloc = "";
    if let Some(after_slashes) = rest.strip_prefix("//") {
        let end = after_slashes.find(['/', '?', '#']).unwrap_or(after_slashes.len());
        netloc = &after_slashes[..end];
        rest = &after_slashes[end..];
        let (open, close) = (netloc.contains('['), netloc.contains(']'));
        if open != close {
            return Err(());
        }
        if open {
            check_bracketed_netloc(netloc)?;
        }
    }
    let path = rest.split(['#', '?']).next().unwrap_or_default().to_string();
    let hostinfo = netloc.rsplit_once('@').map_or(netloc, |(_, h)| h);
    let (hostname, port) = match hostinfo.split_once('[') {
        Some((_, bracketed)) => {
            let (hostname, after) = bracketed.split_once(']').unwrap_or((bracketed, ""));
            (hostname, after.split_once(':').map_or("", |(_, p)| p))
        }
        None => hostinfo.split_once(':').unwrap_or((hostinfo, "")),
    };
    let hostname = (!hostname.is_empty()).then(|| match hostname.split_once('%') {
        Some((name, zone)) => format!("{}%{zone}", name.to_lowercase()),
        None => hostname.to_lowercase(),
    });
    let port = if port.is_empty() {
        None
    } else if !port.chars().all(|c| c.is_ascii_digit()) {
        return Err(());
    } else {
        let digits = port.trim_start_matches('0');
        match digits.parse::<u32>() {
            _ if digits.is_empty() => Some(0),
            Ok(n) if digits.len() <= 5 && n <= 65_535 => Some(n),
            _ => return Err(()),
        }
    };
    Ok(Split { scheme, path, hostname, port })
}

/// `scheme://host[:port]` with an IPv6 host in brackets and a port of 0 left out.
pub(crate) fn origin(parts: &Split) -> String {
    let host = parts.hostname.as_deref().unwrap_or_default();
    let host = if host.contains(':') { format!("[{host}]") } else { host.to_string() };
    let port = parts.port.filter(|p| *p != 0).map(|p| format!(":{p}")).unwrap_or_default();
    format!("{}://{host}{port}", parts.scheme)
}

/// A compiled redactor for one [`Env`].
pub struct Redactor {
    shapes: Vec<(String, Regex)>,
    key_block: Regex,
    auth_header: Regex,
    bearer: Regex,
    flag: Regex,
    pair: Regex,
    json_pair: Regex,
    url: Regex,
    email: Regex,
    other_homes: Regex,
    ipv4: Regex,
    ipv6: Regex,
    home: Option<Regex>,
    hosts: Vec<Regex>,
    user: Option<Regex>,
}

impl Redactor {
    /// Builds the redactor from `secrets.txt` text (use [`SECRETS_TXT`]) and the machine's names.
    pub fn new(secrets: &str, env: Env) -> Self {
        let shapes = Self::shapes(secrets)
            .into_iter()
            .map(|(_, keyword, pattern)| (keyword.to_string(), re(pattern)))
            .collect();
        let home = (!env.home.is_empty()).then(|| re(&regex::escape(&env.home)));
        let mut hosts = Vec::new();
        if env.host.chars().count() >= 3 {
            let short = env.host.split('.').next().unwrap_or_default();
            // The full name first: its short form would otherwise leave `[host].domain`.
            hosts.push(re(&format!("(?i){}", regex::escape(&env.host))));
            if short.chars().count() >= 3 && short != env.host {
                hosts.push(re(&format!("(?i){}", regex::escape(short))));
            }
        }
        let user = (env.user.chars().count() >= 3).then(|| re(&regex::escape(&env.user)));
        Redactor {
            shapes,
            key_block: re(r"(?s)-----BEGIN [A-Z0-9 ]+-----.*?-----END [A-Z0-9 ]+-----"),
            auth_header: re(r"(?i)\bAuthorization\s*:[^\r\n]*"),
            bearer: re(r"(?i)\b(?:Bearer|Basic)\s+[A-Za-z0-9._~+/=-]{8,}"),
            flag: re(r#"(--?[A-Za-z][A-Za-z0-9_-]*)(=|[ \t]+)([^\s"']+|"[^"]*"|'[^']*')"#),
            pair: re(r#"([A-Za-z][A-Za-z0-9_-]*)=("[^"]*"|'[^']*'|[^\s"',;&]+)"#),
            json_pair: re(r#""([A-Za-z][A-Za-z0-9_-]*)"(\s*:\s*)"(?:[^"\\]|\\.)*""#),
            url: re(r#"\b[A-Za-z][A-Za-z0-9+.-]*://[^\s"'<>]+"#),
            email: re(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}"),
            other_homes: re(r#"(?:/home/|/Users/)[^/\s"']+|[A-Za-z]:\\Users\\[^\\\s"']+"#),
            ipv4: re(r"(?:\d{1,3}\.){3}\d{1,3}"),
            ipv6: re(r"(?:[0-9A-Fa-f]{0,4}:){2,7}[0-9A-Fa-f]{0,4}"),
            home,
            hosts,
            user,
        }
    }

    /// The rows of `secrets.txt`: name, keyword, pattern (`#` lines and blank lines skipped).
    pub fn shapes(secrets: &str) -> Vec<(&str, &str, &str)> {
        secrets
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .filter_map(|l| {
                let mut f = l.splitn(3, '|');
                Some((f.next()?, f.next()?, f.next()?))
            })
            .collect()
    }

    /// Runs every replacement of §7.4 rule 3, in order. `scrub_addrs` is true above `debug`.
    pub fn redact(&self, text: &str, scrub_addrs: bool) -> String {
        let mut s = self.key_block.replace_all(text, "[key]").into_owned();
        s = self.auth_header.replace_all(&s, SECRET).into_owned();
        s = self.bearer.replace_all(&s, SECRET).into_owned();
        for (keyword, pattern) in &self.shapes {
            if s.contains(keyword.as_str()) {
                s = pattern.replace_all(&s, SECRET).into_owned();
            }
        }
        let free = |t: &str, at: usize| !before(t, at).is_some_and(|c| is_word(c) || c == '-');
        s = sub_where(
            &self.flag,
            &s,
            |st, _| free(&s, st),
            |c| {
                if secret_like(c[1].trim_start_matches('-')) {
                    format!("{}{}{SECRET}", &c[1], &c[2])
                } else {
                    c[0].to_string()
                }
            },
        );
        s = sub_where(
            &self.pair,
            &s,
            |st, _| free(&s, st),
            |c| if secret_like(&c[1]) { format!("{}={SECRET}", &c[1]) } else { c[0].to_string() },
        );
        s = self
            .json_pair
            .replace_all(&s, |c: &Captures| {
                if secret_like(&c[1]) { format!("\"{}\"{}\"{SECRET}\"", &c[1], &c[2]) } else { c[0].to_string() }
            })
            .into_owned();
        s = self.url.replace_all(&s, |c: &Captures| Self::url_kept(&c[0])).into_owned();
        s = self.email.replace_all(&s, "[email]").into_owned();
        s = self.names(&s);
        if scrub_addrs {
            s = self.addresses(&s);
        }
        s
    }

    fn url_kept(found: &str) -> String {
        let raw = found.trim_end_matches(['.', ',', ';', ')', ']', '}']);
        let trailing = &found[raw.len()..];
        match urlsplit(raw) {
            Ok(parts) => format!("{}{}{trailing}", origin(&parts), parts.path),
            Err(()) => {
                let cut = found.split('?').next().unwrap_or_default();
                let cut = cut.split('#').next().unwrap_or_default();
                cut.rsplit('@').next().unwrap_or_default().to_string()
            }
        }
    }

    fn names(&self, text: &str) -> String {
        let mut s = text.to_string();
        if let Some(home) = &self.home {
            s = sub_name(home, &s, |_| false, is_word, "~");
        }
        s = self.other_homes.replace_all(&s, "~").into_owned();
        for host in &self.hosts {
            s = sub_name(host, &s, |c| is_word(c) || c == '.' || c == '-', |c| is_word(c) || c == '-', "[host]");
        }
        if let Some(user) = &self.user {
            s = sub_name(user, &s, |c| is_word(c) || c == '-', |c| is_word(c) || c == '-', "[user]");
        }
        s
    }

    fn addresses(&self, text: &str) -> String {
        let swap = |c: &Captures| match parse_ip(&c[0]) {
            Some((addr, _)) if !addr.is_loopback() => ADDR.to_string(),
            _ => c[0].to_string(),
        };
        let edge4 = |c: char| is_word(c) || c == '.';
        let s = sub_where(
            &self.ipv4,
            text,
            |st, e| !before(text, st).is_some_and(edge4) && !after(text, e).is_some_and(edge4),
            swap,
        );
        let edge6 = |c: char| is_word(c) || c == ':';
        sub_where(
            &self.ipv6,
            &s,
            |st, e| !before(&s, st).is_some_and(edge6) && !after(&s, e).is_some_and(edge6),
            swap,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlsplit_follows_python() {
        let p = urlsplit("HTTPS://User@Host.Example:0443/a/b?q#f").unwrap();
        assert_eq!((p.scheme.as_str(), p.hostname.as_deref(), p.port, p.path.as_str()), ("https", Some("host.example"), Some(443), "/a/b"));
        assert!(urlsplit("http://[::1").is_err());
        assert!(urlsplit("http://[127.0.0.1]/").is_err());
        assert!(urlsplit("http://h:99999/").is_err());
        assert!(urlsplit("http://h:x/").is_err());
        assert_eq!(urlsplit("http://[FE80::1%eTh0]:80/").unwrap().hostname.as_deref(), Some("fe80::1%eTh0"));
    }

    #[test]
    fn ipv6_text_is_pythons() {
        let t = |s: &str| {
            let (a, z) = parse_ip(s).unwrap();
            ip_text(a, z)
        };
        assert_eq!(t("2001:DB8:0:0:1:0:0:1"), "2001:db8::1:0:0:1");
        assert_eq!(t("1:0:2:3:4:5:6:7"), "1:0:2:3:4:5:6:7");
        assert_eq!(t("::ffff:1.2.3.4"), "::ffff:102:304");
        assert_eq!(t("::"), "::");
    }

    #[test]
    fn a_look_around_rejects_and_retries() {
        let r = Redactor::new("", Env::new("", "", ""));
        assert_eq!(r.redact("x10.0.0.1 10.0.0.1 10.0.0.1.5", true), "x10.0.0.1 [addr] 10.0.0.1.5");
        assert_eq!(r.redact("1api_key=2 api_key=3", true), "1api_key=2 api_key=[secret]");
    }
}
