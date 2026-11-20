//! What a command is, read from its text: whether it reads files or searches code (its output then
//! reaches the model as before, answer 29), and its kind (the program and its subcommand), which
//! the switch-off of answer 50 counts per. A plain split on `&&`, `||`, `;`, `|` and newlines; quotes
//! are not parsed, which errs towards passing an output through.

/// Programs whose output is file contents, a listing or search hits.
const READERS: &[&str] = &[
    "cat", "head", "tail", "less", "more", "sed", "awk", "gawk", "grep", "egrep", "fgrep", "rg", "ag", "ack", "find",
    "fd", "ls", "dir", "tree", "nl", "bat", "wc", "jq", "yq", "diff", "cmp", "xxd", "od", "hexdump", "strings", "file",
    "stat", "du", "sort", "uniq", "cut", "column", "get-content", "gc", "type", "select-string", "get-childitem",
];

/// Git subcommands that show history, contents or diffs.
const GIT_READERS: &[&str] = &["diff", "show", "log", "blame", "grep", "ls-files", "ls-tree", "cat-file", "reflog"];

/// Words that only set up the command that follows them.
const SETUP: &[&str] = &["cd", "pushd", "popd", "export", "source", ".", "set", "true", "echo", "unset", "ulimit"];

/// Wrappers whose first non-option argument is the real program.
const WRAPPERS: &[&str] = &["sudo", "env", "time", "nice", "nohup", "command", "exec", "stdbuf", "ionice", "xvfb-run"];

/// Programs named together with their first non-option argument (`cargo test`, `npm run`).
const WITH_SUBCOMMAND: &[&str] = &[
    "cargo", "npm", "pnpm", "yarn", "npx", "bun", "deno", "go", "git", "make", "uv", "poetry", "pip", "pip3", "dotnet",
    "mvn", "gradle", "gradlew", "docker", "podman", "kubectl", "just", "tox", "nox", "bazel", "swift", "zig", "ragon",
];

fn segments(command: &str) -> impl Iterator<Item = &str> {
    command.split(['\n', ';', '|', '&']).map(str::trim).filter(|segment| !segment.is_empty())
}

fn program_name(word: &str) -> String {
    let base = word.rsplit(['/', '\\']).next().unwrap_or(word);
    let base = base.strip_suffix(".exe").unwrap_or(base);
    base.trim_matches(['"', '\'', '(', ')', '{', '}']).to_lowercase()
}

/// The words of one segment from its program on: assignments and wrappers skipped.
fn program_words(segment: &str) -> Vec<&str> {
    let words: Vec<&str> = segment.split_whitespace().collect();
    let mut index = 0;
    while index < words.len() {
        let word = words[index];
        let name = program_name(word);
        if word.contains('=') && !word.starts_with('-') && !word.starts_with('=') {
            index += 1;
        } else if name == "timeout" {
            // `timeout [options] DURATION program ...`; `-s` and `-k` take a value.
            index += 1;
            while index < words.len() && words[index].starts_with('-') {
                let takes_value = matches!(words[index], "-s" | "--signal" | "-k" | "--kill-after");
                index += if takes_value { 2 } else { 1 };
            }
            index += 1;
        } else if WRAPPERS.contains(&name.as_str()) {
            index += 1;
            while index < words.len() && words[index].starts_with('-') {
                index += 1;
            }
        } else {
            break;
        }
    }
    words[index.min(words.len())..].to_vec()
}

/// The first argument of `words[0]` that is not an option (`git -C dir diff` gives `diff`).
fn subcommand<'a>(words: &[&'a str]) -> Option<&'a str> {
    let mut rest = words.iter().skip(1);
    while let Some(word) = rest.next() {
        if matches!(*word, "-C" | "-c" | "--manifest-path" | "-p" | "--prefix" | "-f" | "--file") {
            rest.next();
        } else if !word.starts_with('-') && !word.starts_with('+') {
            return Some(word);
        }
    }
    None
}

/// Whether some part of the command reads files, lists them or searches code.
pub fn reads(command: &str) -> bool {
    segments(command).any(|segment| {
        let words = program_words(segment);
        let Some(first) = words.first() else { return false };
        let name = program_name(first);
        READERS.contains(&name.as_str())
            || (name == "git" && subcommand(&words).is_some_and(|sub| GIT_READERS.contains(&sub)))
    })
}

/// The command's kind: its first real program, with the subcommand for tools that have them, and
/// the module for `python -m`.
pub fn kind(command: &str) -> String {
    for segment in segments(command) {
        let words = program_words(segment);
        let Some(first) = words.first() else { continue };
        let name = program_name(first);
        if SETUP.contains(&name.as_str()) {
            continue;
        }
        if name.starts_with("python") || name == "py" {
            if let Some(position) = words.iter().position(|word| *word == "-m") {
                if let Some(module) = words.get(position + 1) {
                    return format!("{name} -m {}", program_name(module));
                }
            }
            if let Some(script) = words.iter().skip(1).find(|word| !word.starts_with('-')) {
                return format!("{name} {}", program_name(script));
            }
            return name;
        }
        if WITH_SUBCOMMAND.contains(&name.as_str()) {
            if let Some(sub) = subcommand(&words) {
                return format!("{name} {}", program_name(sub));
            }
        }
        return name;
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reading_and_searching_commands_pass_through() {
        for command in [
            "cat src/lib.rs",
            "sed -n '1,80p' x.py",
            "rg -n foo",
            "cd repo && git -C . diff HEAD~1",
            "cargo test 2>&1 | tail -n 50",
            "FOO=1 timeout 60 grep -r x .",
            "Get-Content x.log",
        ] {
            assert!(reads(command), "{command}");
        }
        for command in ["cargo build --release", "pytest -q", "npm run build", "git commit -m x", "make -j8"] {
            assert!(!reads(command), "{command}");
        }
    }

    #[test]
    fn the_kind_is_the_program_and_its_subcommand() {
        assert_eq!(kind("cd /repo && cargo test --release -p x"), "cargo test");
        assert_eq!(kind("RUST_LOG=debug timeout -s KILL 600 cargo +nightly build"), "cargo build");
        assert_eq!(kind("python -m pytest -q tests/"), "python -m pytest");
        assert_eq!(kind("/usr/bin/python3 setup.py build"), "python3 setup.py");
        assert_eq!(kind("pytest -x"), "pytest");
        assert_eq!(kind("npm run test"), "npm run");
        assert_eq!(kind("export A=1; make -C build all"), "make all");
    }
}
