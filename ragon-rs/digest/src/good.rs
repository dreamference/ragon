//! The last good run (owner's answer 2 of 2026-11-20): line samples of the last successful run of
//! the same command, so that a digest can leave out the lines that run printed too. No model takes
//! part. A sample is a hash of the line with its leading timestamp removed and every run of digits
//! made one `#`, so a line that differs only in a time, a count or a duration still matches.
//!
//! The samples of one command are at most [`MAX_SAMPLES`] hashes (the smallest ones, so a bounded
//! sample of a long output is the same subset each time), kept in `digest/good/` for a day like the
//! raw outputs.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// The most line hashes kept for one command: 8 bytes each, so 160 KB at most.
pub const MAX_SAMPLES: usize = 20_000;

/// The most the samples of all commands may take together; the oldest go first.
pub const CAP_BYTES: u64 = 32 * 1024 * 1024;

/// FNV-1a, 64 bits: stable across builds and platforms, unlike the standard library's hasher.
pub fn fnv(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// Whether `word` is a time stamp: digits with at least two of `-`, `:`, `.`, `T`, `Z`, `/`, `+`,
/// `,` (`2026-11-20T21:48:03.123Z`, `21:48:03`, `[12:01:02]`).
fn stamp(word: &str) -> bool {
    let word = word.trim_start_matches('[').trim_end_matches([']', ',']);
    let digits = word.chars().filter(char::is_ascii_digit).count();
    let marks = word.chars().filter(|c| matches!(c, '-' | ':' | '.' | 'T' | 'Z' | '/' | '+' | ',')).count();
    digits >= 4 && marks >= 2 && word.chars().all(|c| c.is_ascii_digit() || matches!(c, '-' | ':' | '.' | 'T' | 'Z' | '/' | '+' | ','))
}

/// A cleaned line as it is compared: leading time stamps removed, every run of digits one `#`,
/// spaces at both ends trimmed.
pub fn normalize(line: &str) -> String {
    let mut rest = line.trim_start();
    for _ in 0..2 {
        match rest.split_once(char::is_whitespace) {
            Some((first, after)) if stamp(first) => rest = after.trim_start(),
            _ => break,
        }
    }
    let mut out = String::with_capacity(rest.len());
    let mut in_digits = false;
    for c in rest.trim_end().chars() {
        if c.is_ascii_digit() {
            if !in_digits {
                out.push('#');
            }
            in_digits = true;
        } else {
            out.push(c);
            in_digits = false;
        }
    }
    out
}

/// The hash a line is sampled and looked up by; `None` for a blank line, which is never left out.
pub fn line_hash(line: &str) -> Option<u64> {
    let normal = normalize(line);
    (!normal.is_empty()).then(|| fnv(normal.as_bytes()))
}

/// The bounded sample of `lines`: the [`MAX_SAMPLES`] smallest distinct line hashes, sorted.
pub fn sample(lines: &[String]) -> Vec<u64> {
    let mut hashes: Vec<u64> = lines.iter().filter_map(|line| line_hash(line)).collect();
    hashes.sort_unstable();
    hashes.dedup();
    hashes.truncate(MAX_SAMPLES);
    hashes
}

/// The samples of one command's last good run.
#[derive(Debug, Default)]
pub struct Samples {
    hashes: HashSet<u64>,
    /// The largest hash kept when the sample was cut at [`MAX_SAMPLES`]: a line hashing above it
    /// may have been printed and is therefore never treated as known.
    ceiling: Option<u64>,
}

impl Samples {
    /// Samples from a sorted list as [`sample`] gives it.
    pub fn from_sorted(hashes: &[u64]) -> Self {
        let ceiling = (hashes.len() >= MAX_SAMPLES).then(|| hashes.last().copied()).flatten();
        Self { hashes: hashes.iter().copied().collect(), ceiling }
    }

    /// Whether `line` was printed by the last good run, as far as the sample can tell.
    pub fn knows(&self, line: &str) -> bool {
        line_hash(line).is_some_and(|hash| self.ceiling.is_none_or(|top| hash <= top) && self.hashes.contains(&hash))
    }

    /// For each line, whether the last good run printed it.
    pub fn mark(&self, lines: &[String]) -> Vec<bool> {
        lines.iter().map(|line| self.knows(line)).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.hashes.is_empty()
    }
}

/// The file the samples of `command` are kept in, under `dir` (`digest/good`).
pub fn path(dir: &Path, command: &str) -> PathBuf {
    dir.join(format!("{:016x}.bin", fnv(command.trim().as_bytes())))
}

/// Keeps `lines` as the last good run of `command`, replacing the one before.
pub fn record(dir: &Path, command: &str, lines: &[String]) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let bytes: Vec<u8> = sample(lines).iter().flat_map(|hash| hash.to_le_bytes()).collect();
    let path = path(dir, command);
    let partial = path.with_extension("part");
    fs::write(&partial, bytes)?;
    fs::rename(&partial, &path)
}

/// The samples of the last good run of `command`, if one was kept and is younger than a day.
pub fn load(dir: &Path, command: &str, now: std::time::SystemTime) -> Option<Samples> {
    let path = path(dir, command);
    let modified = path.metadata().ok()?.modified().ok()?;
    if now.duration_since(modified).unwrap_or_default() > crate::KEEP_FOR {
        return None;
    }
    let bytes = fs::read(&path).ok()?;
    let hashes: Vec<u64> =
        bytes.chunks_exact(8).map(|chunk| u64::from_le_bytes(chunk.try_into().unwrap_or_default())).collect();
    let samples = Samples::from_sorted(&hashes);
    (!samples.is_empty()).then_some(samples)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `i` written in letters, so that distinct numbers stay distinct lines after digits are masked.
    fn letters(mut i: usize) -> String {
        let mut out = String::new();
        loop {
            out.push(char::from(b'a' + (i % 26) as u8));
            i /= 26;
            if i == 0 {
                return out;
            }
        }
    }

    #[test]
    fn times_counts_and_durations_do_not_make_a_line_new() {
        assert_eq!(normalize("2026-11-20T21:48:03.1234567Z   Compiling serde v1.0.215"), "Compiling serde v#.#.#");
        assert_eq!(normalize("[21:48:03] took 12ms"), "took #ms");
        assert_eq!(normalize("  test result: ok. 776 passed; 0 failed  "), "test result: ok. # passed; # failed");
        assert_eq!(normalize("2026 errors"), "# errors");
        assert_eq!(line_hash("   "), None);
    }

    #[test]
    fn a_long_output_is_sampled_within_the_bound_and_the_same_subset_each_time() {
        let lines: Vec<String> = (0..MAX_SAMPLES * 2).map(|i| format!("line {}", letters(i))).collect();
        let first = sample(&lines);
        assert_eq!(first.len(), MAX_SAMPLES);
        assert_eq!(first, sample(&lines));
        let samples = Samples::from_sorted(&first);
        let known = samples.mark(&lines).iter().filter(|known| **known).count();
        assert_eq!(known, MAX_SAMPLES, "only sampled lines count as known");
    }
}
