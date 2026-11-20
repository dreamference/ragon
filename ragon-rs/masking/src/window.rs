//! Hiding old outputs (owner's answer 3 of 2026-11-20, specs/DREAMFERENCE_RAGON_OUTPUT_PREPROCESSING.md
//! §2.6): every tool output older than the last [`KEEP_STEPS`] steps is replaced in what is sent by
//! one line naming where the whole output is. A step is one tool call and its output. When the
//! output is a digest, the line names the digest's raw file; otherwise the output is saved once to
//! `$CODEX_HOME/masking/hidden/` and the line names that copy.
//!
//! Separate from size-triggered masking and off by default: nothing happens until the launcher
//! calls [`set_window`]. The rule depends only on the history, so a resumed session sends the same
//! view. Like masking, it changes what is sent, never what the rollout stores.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::SystemTime;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::{call_id, file_name, is_output, older_than_kept, output_text};

/// The most recent outputs that are always sent whole.
pub const KEEP_STEPS: usize = 10;

/// What the hide-old rule keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    /// Outputs with fewer than this many outputs after them are sent whole.
    pub keep_steps: usize,
}

impl Default for Window {
    fn default() -> Self {
        Self { keep_steps: KEEP_STEPS }
    }
}

struct Setup {
    window: Window,
    dir: PathBuf,
}

static SETUP: OnceLock<Setup> = OnceLock::new();

/// Switches hiding old outputs on for this process, keeping copies under `codex_home`. The first
/// call wins; returns whether it was this one.
pub fn set_window(window: Window, codex_home: &Path) -> bool {
    SETUP.set(Setup { window, dir: hidden_dir(codex_home) }).is_ok()
}

/// The window in force, if any.
pub fn window() -> Option<&'static Window> {
    SETUP.get().map(|setup| &setup.window)
}

/// Where copies of hidden outputs that are not digests are kept.
pub fn hidden_dir(codex_home: &Path) -> PathBuf {
    codex_home.join("masking").join("hidden")
}

/// Hides old outputs under the process's window; does nothing without one.
pub(crate) fn apply<E, T>(items: &mut [E], item: &impl Fn(&mut E) -> &mut T)
where
    T: Serialize + DeserializeOwned,
{
    if let Some(setup) = SETUP.get() {
        hide_with(&setup.window, &setup.dir, items, item);
    }
}

/// The raw file a digest names (`the full output is in <path> (read lines …`).
pub fn raw_path(text: &str) -> Option<PathBuf> {
    let start = text.find("the full output is in ")? + "the full output is in ".len();
    let rest = &text[start..];
    let end = rest.find(" (read lines")?;
    Some(PathBuf::from(&rest[..end])).filter(|path| !path.as_os_str().is_empty())
}

/// The one line an old output becomes.
pub fn line(text: &str, path: &Path) -> String {
    format!("[old output hidden, {} chars; the whole output is in {}]", text.chars().count(), path.display())
}

/// The indices of the outputs that have at least `window.keep_steps` outputs after them.
pub fn plan(window: &Window, values: &[Value]) -> Vec<usize> {
    let outputs: Vec<usize> =
        values.iter().enumerate().filter(|(_, value)| is_output(value)).map(|(index, _)| index).collect();
    let old = outputs.len().saturating_sub(window.keep_steps);
    outputs[..old].to_vec()
}

/// [`apply`] with an explicit window and folder. An output whose line would not be shorter, or
/// whose copy cannot be written, is sent as it was.
pub fn hide_with<E, T>(window: &Window, dir: &Path, items: &mut [E], item: &impl Fn(&mut E) -> &mut T)
where
    T: Serialize + DeserializeOwned,
{
    let values: Vec<Value> =
        items.iter_mut().map(|entry| serde_json::to_value(&*item(entry)).unwrap_or(Value::Null)).collect();
    for index in plan(window, &values) {
        let value = &values[index];
        let Some(id) = call_id(value) else { continue };
        let text = output_text(value);
        let path = match raw_path(&text) {
            Some(path) => path,
            None => copy_path(dir, id),
        };
        let hidden = line(&text, &path);
        if hidden.chars().count() >= text.chars().count() {
            continue;
        }
        if raw_path(&text).is_none() && save(&path, &text).is_err() {
            continue;
        }
        let Value::Object(fields) = value else { continue };
        let mut fields = fields.clone();
        fields.insert("output".to_string(), Value::String(hidden));
        if let Ok(replaced) = serde_json::from_value::<T>(Value::Object(fields)) {
            *item(&mut items[index]) = replaced;
        }
    }
}

/// Where the copy of a hidden output lives.
pub fn copy_path(dir: &Path, call_id: &str) -> PathBuf {
    dir.join(format!("{}.txt", file_name(call_id)))
}

fn save(path: &Path, text: &str) -> std::io::Result<()> {
    if path.is_file() {
        return Ok(());
    }
    std::fs::create_dir_all(path.parent().unwrap_or(path))?;
    let staging = path.with_extension("txt.tmp");
    std::fs::write(&staging, text)?;
    std::fs::rename(staging, path)
}

/// Deletes copies of hidden outputs older than the masking files' retention. Returns how many.
pub fn prune(codex_home: &Path, now: SystemTime) -> usize {
    let Ok(entries) = std::fs::read_dir(hidden_dir(codex_home)) else { return 0 };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| older_than_kept(path, now) && std::fs::remove_file(path).is_ok())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ragon-window-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn history(calls: usize, output: impl Fn(usize) -> String) -> Vec<Value> {
        let mut out = vec![json!({"type": "message", "role": "user", "content": [{"type": "input_text", "text": "fix it"}]})];
        for n in 0..calls {
            let id = format!("call_{n:03}");
            out.push(json!({"type": "function_call", "name": "exec_command", "arguments": "{\"cmd\":\"make\"}", "call_id": id}));
            out.push(json!({"type": "function_call_output", "call_id": id, "output": output(n)}));
        }
        out
    }

    fn long(n: usize) -> String {
        format!("Process exited with code 0\nOutput:\n{}", format!("line of output {n}\n").repeat(40))
    }

    #[test]
    fn outputs_older_than_the_last_ten_steps_become_one_line_naming_a_saved_copy() {
        let dir = scratch("copy");
        let mut items = history(15, long);
        let before = items.clone();
        hide_with(&Window::default(), &dir, &mut items, &|value: &mut Value| value);
        for (index, (now, was)) in items.iter().zip(&before).enumerate() {
            let old_output = index % 2 == 0 && index > 0 && index / 2 <= 5;
            if !old_output {
                assert_eq!(now, was, "item {index} is unchanged");
                continue;
            }
            let text = now["output"].as_str().unwrap();
            assert_eq!(text.lines().count(), 1, "{text}");
            let path = copy_path(&dir, now["call_id"].as_str().unwrap());
            assert!(text.contains(&path.display().to_string()), "{text}");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), was["output"].as_str().unwrap());
        }
        // The same history gives the same view again (a resumed session, the prefix cache).
        let mut again = before.clone();
        hide_with(&Window::default(), &dir, &mut again, &|value: &mut Value| value);
        assert_eq!(again, items);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_digest_is_hidden_behind_its_raw_file_and_a_short_output_stays() {
        let dir = scratch("digest");
        let digest = "[digest] 900 lines, exit code 1. Shown: …; the full output is in /tmp/digest/raw/0123.log (read lines with: sed -n '1,40p' '/tmp/digest/raw/0123.log').\n12: error: boom\n".repeat(3);
        let mut items = history(12, |n| match n {
            0 => digest.clone(),
            1 => "ok".to_string(),
            _ => long(n),
        });
        hide_with(&Window::default(), &dir, &mut items, &|value: &mut Value| value);
        assert_eq!(items[2]["output"], json!(line(&digest, Path::new("/tmp/digest/raw/0123.log"))));
        assert_eq!(items[4]["output"], json!("ok"));
        assert!(!dir.exists(), "a digest and a short output need no copy");
    }

    #[test]
    fn ten_outputs_or_fewer_are_all_sent_whole() {
        let dir = scratch("few");
        let mut items = history(10, long);
        let before = items.clone();
        hide_with(&Window::default(), &dir, &mut items, &|value: &mut Value| value);
        assert_eq!(items, before);
        assert!(window().is_none(), "off unless the launcher sets it");
    }
}
