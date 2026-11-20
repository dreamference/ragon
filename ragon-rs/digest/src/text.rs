//! Turning raw bytes into the lines the digest selects from: one line per `\n`, as `sed -n` counts
//! them in the raw file, with colours, cursor moves and other control characters removed. A bare
//! `\r` is a progress bar redrawing its line, so only the text after the last one is kept.

/// The output's lines, cleaned. Line `i` here is line `i + 1` of the raw file.
pub fn lines(raw: &[u8]) -> Vec<String> {
    let body = raw.strip_suffix(b"\n").unwrap_or(raw);
    if raw.is_empty() {
        return Vec::new();
    }
    body.split(|byte| *byte == b'\n').map(|line| clean(&String::from_utf8_lossy(line))).collect()
}

/// The number of lines `lines` returns, without building them.
pub fn line_count(raw: &[u8]) -> usize {
    if raw.is_empty() {
        return 0;
    }
    let newlines = raw.iter().filter(|byte| **byte == b'\n').count();
    if raw.ends_with(b"\n") { newlines } else { newlines + 1 }
}

/// Whether the output is binary rather than text: a NUL byte, or more than a tenth of it not UTF-8.
pub fn is_binary(raw: &[u8]) -> bool {
    if raw.contains(&0) {
        return true;
    }
    let invalid: usize = raw.utf8_chunks().map(|chunk| chunk.invalid().len()).sum();
    invalid * 10 > raw.len()
}

/// One line without escape sequences and control characters; tabs stay.
pub fn clean(line: &str) -> String {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let line = match line.rsplit_once('\r') {
        Some((before, after)) if after.trim().is_empty() => before.rsplit('\r').next().unwrap_or(before),
        Some((_, after)) => after,
        None => line,
    };
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.next() {
                // CSI: parameters and intermediates up to a final byte in @..~.
                Some('[') => {
                    for next in chars.by_ref() {
                        if ('@'..='~').contains(&next) {
                            break;
                        }
                    }
                }
                // OSC: up to BEL or ESC \.
                Some(']') => {
                    while let Some(next) = chars.next() {
                        if next == '\u{7}' {
                            break;
                        }
                        if next == '\u{1b}' {
                            chars.next_if_eq(&'\\');
                            break;
                        }
                    }
                }
                // A two-character escape (charset selection and the like).
                Some('(' | ')' | '#' | '%') => {
                    chars.next();
                }
                _ => {}
            }
        } else if c == '\t' || !c.is_control() {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_and_progress_redraws_are_removed() {
        assert_eq!(clean("\u{1b}[1m\u{1b}[91merror\u{1b}[0m: x"), "error: x");
        assert_eq!(clean("10%\r50%\r100% done\r\n"), "100% done");
        assert_eq!(clean("\u{1b}]0;title\u{7}text"), "text");
        assert_eq!(clean("a\tb\u{8}"), "a\tb");
    }

    #[test]
    fn lines_are_counted_as_sed_counts_them() {
        assert_eq!(line_count(b""), 0);
        assert_eq!(line_count(b"a"), 1);
        assert_eq!(line_count(b"a\n"), 1);
        assert_eq!(line_count(b"a\nb"), 2);
        assert_eq!(lines(b"a\n\nb\n"), vec!["a", "", "b"]);
        assert_eq!(lines(b"x\r\ny\r\n").len(), line_count(b"x\r\ny\r\n"));
    }

    #[test]
    fn binary_is_nul_or_mostly_not_utf8() {
        assert!(is_binary(b"ab\0cd"));
        assert!(is_binary(&[0xff; 64]));
        assert!(!is_binary("ошибка: 错误\n".as_bytes()));
    }
}
