//! The launcher's calls to `ragon-docs`, the local file index
//! (specs/DREAMFERENCE_RAGON_LOCAL_INDEX.md §6, §8). Like the code index (code_index.rs), the
//! index is a program of its own and this is all the launcher knows about it:
//!
//! 1. `ragon docs <command>` runs `ragon-docs <command>` (`ragon docs add|remove|search|read|status`);
//! 2. at a session's start it starts `ragon-docs session --parent-pid <this process>` detached,
//!    outside the sandbox, which keeps every collection indexed;
//! 3. it appends what `ragon-docs prompt-block --tools` prints, and when that block names a
//!    collection (`# Your documents`) or PDF search (`# PDFs`, the run-time files are installed:
//!    specs/DREAMFERENCE_PDF_SEARCH.md §3.2) it offers the tools through the `-c mcp_servers.…`
//!    arguments that make Codex start `ragon-docs mcp` for the session. The server lists what
//!    applies: `docs_search` with a collection, `pdf_search` with PDFium, `docs_read` with either.
//!
//! With `ragon-docs` not installed, or `ragon_docs = false`, all of it is skipped. Every
//! collection is visible to every session (§14.2), so nothing here depends on the directory.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Where `ragon-admin codex build` (or a release install) puts `ragon-docs`: beside `ragon`, never
/// looked up on PATH. Linux only: the index runs in systemd scopes inside bwrap, so no other
/// system has it, and there this finds nothing.
pub fn binary() -> Option<PathBuf> {
    let path = match std::env::var_os("RAGON_DOCS_BIN") {
        Some(path) => PathBuf::from(path),
        None => ragon_node_locator::home_dir()?.join(".local/share/dreamference/ragon/bin/ragon-docs"),
    };
    path.is_file().then_some(path)
}

/// The MCP server's name in Codex's configuration; its tools reach the model as `docs_search`,
/// `docs_read` and `pdf_search` (patch 0020).
pub const MCP_SERVER: &str = "ragon_docs";

/// What `ragon-docs mcp` must see beyond Codex's short allow-list: where the home, the settings
/// and the installed model are when they are not in their default places; every
/// `DREAMFERENCE_RAGON_DOCS_*` setting `ragon-docs-rs/src/config.rs` reads (the embedding
/// model above all: a query embedded with another model than the index's finds nothing); for `pdf_search`
/// (PDF_SEARCH §3.2), the user's systemd and the shared admission ledger (a scope cannot be
/// started without them, and admission would keep a ledger of its own), the air-gap level and
/// the proxy settings `ragon-fetch` downloads with.
const FORWARDED_ENV: &[&str] = &[
    "CODEX_HOME",
    "DREAMFERENCE_CONFIG_PATH",
    "DREAMFERENCE_RAGON_DOCS",
    "DREAMFERENCE_RAGON_DOCS_BM25_WEIGHT",
    "DREAMFERENCE_RAGON_DOCS_EMBEDDING_MODEL",
    "DREAMFERENCE_RAGON_DOCS_SCAN_INTERVAL_MIN",
    "DREAMFERENCE_RAGON_DOCS_MAX_INDEX_GB",
    "DREAMFERENCE_RAGON_DOCS_EXTRACT_TIMEOUT_S",
    "DREAMFERENCE_RAGON_DOCS_OCR",
    "DREAMFERENCE_RAGON_DOCS_OCR_MAX_PAGES",
    "DREAMFERENCE_RAGON_DOCS_OCR_TIMEOUT_S",
    "DREAMFERENCE_RAGON_DOCS_OCR_SEND_BELOW",
    "RAGON_DOCS_OCR_DIR",
    "RAGON_DOCS_LIB_DIR",
    "RAGON_DOCS_MODEL_DIR",
    "XDG_RUNTIME_DIR",
    "DBUS_SESSION_BUS_ADDRESS",
    "DREAMFERENCE_RAGON_AIRGAPPED",
    "DREAMFERENCE_RAGON_DOCS_PDF_MAX_MB",
    "DREAMFERENCE_RAGON_DOCS_PDF_MAX_PAGES",
    "DREAMFERENCE_RAGON_DOCS_PDF_TIMEOUT_S",
    "DREAMFERENCE_RAGON_DOCS_PDF_WAIT_S",
    "DREAMFERENCE_RAGON_DOCS_PDF_CACHE_MB",
    "DREAMFERENCE_RAGON_DOCS_PDF_CACHE_DAYS",
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "no_proxy",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
];

/// Whether a session's docs block offers tools: a collection, or PDF search.
fn offers_tools(block: &str) -> bool {
    block.contains("# Your documents") || block.contains("# PDFs")
}

/// `ragon_docs`: the environment's `DREAMFERENCE_RAGON_DOCS`, then the config file, then on.
pub fn enabled(config_file: Option<PathBuf>) -> bool {
    if let Ok(setting) = std::env::var("DREAMFERENCE_RAGON_DOCS")
        && !setting.is_empty()
    {
        return !matches!(setting.to_lowercase().as_str(), "0" | "false" | "no" | "off");
    }
    config_file
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| text.parse::<toml::Table>().ok())
        .and_then(|table| table.get("ragon_docs")?.as_bool())
        .unwrap_or(true)
}

/// `ragon docs …`: runs `ragon-docs …` and returns its exit code.
pub fn run_cli(args: &[String]) -> i32 {
    let Some(binary) = binary() else {
        eprintln!("The local file index (ragon-docs) is not installed: a release installs it on Linux, and `ragon-admin codex build` builds it.");
        return 1;
    };
    match Command::new(&binary).args(args).status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            eprintln!("cannot run {}: {error}", binary.display());
            1
        }
    }
}

/// Starts the session process and returns the prompt block (empty when there is none).
pub fn start_and_prompt_block(enabled: bool) -> String {
    let Some(binary) = binary().filter(|_| enabled) else { return String::new() };
    // Its own process group, so Ctrl-C in the TUI does not reach it; it exits with this process.
    let mut command = Command::new(&binary);
    command
        .args(["session", "--parent-pid", &std::process::id().to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let _ = command.spawn();
    block_from(&binary)
}

/// The prompt block alone, starting nothing.
pub fn prompt_block(enabled: bool) -> String {
    binary().filter(|_| enabled).map(|binary| block_from(&binary)).unwrap_or_default()
}

fn block_from(binary: &std::path::Path) -> String {
    let Ok(output) = Command::new(binary).args(["prompt-block", "--tools"]).stdin(Stdio::null()).stderr(Stdio::null()).output() else {
        return String::new();
    };
    let block = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !output.status.success() || block.is_empty() {
        return String::new();
    }
    format!("\n\n{block}\n")
}

/// Adds the arguments that make Codex start `ragon-docs mcp`, when `block` (the session's docs
/// block) says a collection exists or PDFs can be searched. A `-c mcp_servers.ragon_docs…` of the
/// user's own wins.
pub fn with_tools(args: Vec<OsString>, block: &str) -> Vec<OsString> {
    if !offers_tools(block) {
        return args;
    }
    let Some(binary) = binary() else { return args };
    let key = format!("mcp_servers.{MCP_SERVER}.");
    if args.iter().any(|arg| arg.to_string_lossy().contains(&key)) {
        return args;
    }
    let quoted = |text: &str| toml::Value::String(text.to_string()).to_string();
    let forwarded: Vec<String> = FORWARDED_ENV.iter().map(|name| quoted(name)).collect();
    let settings = [
        format!("{key}command={}", quoted(&binary.to_string_lossy())),
        format!("{key}args=[\"mcp\"]"),
        format!("{key}env_vars=[{}]", forwarded.join(", ")),
    ];
    let mut args = args.into_iter();
    let mut out: Vec<OsString> = args.next().into_iter().collect();
    for setting in settings {
        out.extend(["-c".into(), OsString::from(setting)]);
    }
    out.extend(args);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    static BIN_ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn without_ragon_docs_nothing_is_added() {
        let _turn = BIN_ENV.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        // SAFETY: the tests of this module are the only readers of RAGON_DOCS_BIN.
        unsafe { std::env::set_var("RAGON_DOCS_BIN", "/nonexistent/ragon-docs") };
        assert_eq!(start_and_prompt_block(true), "");
        let args: Vec<OsString> = vec!["ragon".into(), "exec".into()];
        assert_eq!(with_tools(args.clone(), "\n\n# Your documents\n"), args);
        assert_eq!(run_cli(&["status".to_string()]), 1);
        unsafe { std::env::remove_var("RAGON_DOCS_BIN") };
    }

    #[test]
    fn the_tools_are_offered_only_with_a_collection_and_the_users_setting_wins() {
        let _turn = BIN_ENV.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = std::env::temp_dir().join(format!("ragon-docs-tools-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap_or_default();
        let binary = dir.join("ragon-docs");
        std::fs::write(&binary, "").unwrap_or_default();
        // SAFETY: the tests of this module are the only readers of RAGON_DOCS_BIN.
        unsafe { std::env::set_var("RAGON_DOCS_BIN", &binary) };
        let base: Vec<OsString> = vec!["ragon".into(), "exec".into(), "hi".into()];
        assert_eq!(with_tools(base.clone(), ""), base, "no collection, no tools");
        let out: Vec<String> = with_tools(base.clone(), "\n\n# Your documents\n\nx").iter().map(|a| a.to_string_lossy().into_owned()).collect();
        // The path as TOML writes it, as code_index.rs's test has it: a Windows path holds
        // backslashes and comes out as a literal string ('C:\…'), not a basic one ("…").
        assert_eq!(out[2], format!("mcp_servers.ragon_docs.command={}", toml::Value::String(binary.display().to_string())));
        assert_eq!(out[4], "mcp_servers.ragon_docs.args=[\"mcp\"]");
        assert!(out[6].starts_with("mcp_servers.ragon_docs.env_vars=[\"CODEX_HOME\""), "{}", out[6]);
        // Every setting ragon-docs reads from its environment (ragon-docs-rs/src/config.rs).
        for name in [
            "DREAMFERENCE_RAGON_DOCS",
            "DREAMFERENCE_RAGON_DOCS_BM25_WEIGHT",
            "DREAMFERENCE_RAGON_DOCS_EMBEDDING_MODEL",
            "DREAMFERENCE_RAGON_DOCS_SCAN_INTERVAL_MIN",
            "DREAMFERENCE_RAGON_DOCS_MAX_INDEX_GB",
            "DREAMFERENCE_RAGON_DOCS_EXTRACT_TIMEOUT_S",
    "DREAMFERENCE_RAGON_DOCS_OCR",
    "DREAMFERENCE_RAGON_DOCS_OCR_MAX_PAGES",
    "DREAMFERENCE_RAGON_DOCS_OCR_TIMEOUT_S",
    "DREAMFERENCE_RAGON_DOCS_OCR_SEND_BELOW",
    "RAGON_DOCS_OCR_DIR",
            "DREAMFERENCE_RAGON_DOCS_PDF_MAX_MB",
            "DREAMFERENCE_RAGON_DOCS_PDF_MAX_PAGES",
            "DREAMFERENCE_RAGON_DOCS_PDF_TIMEOUT_S",
            "DREAMFERENCE_RAGON_DOCS_PDF_WAIT_S",
            "DREAMFERENCE_RAGON_DOCS_PDF_CACHE_MB",
            "DREAMFERENCE_RAGON_DOCS_PDF_CACHE_DAYS",
        ] {
            assert!(out[6].contains(&format!("\"{name}\"")), "{name}: {}", out[6]);
        }
        assert_eq!(&out[7..], ["exec", "hi"]);
        // PDF search alone (no collection) offers the server too, with what its runs need.
        let pdf: Vec<String> = with_tools(base.clone(), "\n\n# PDFs\n\nTo search a long PDF").iter().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(pdf[4], "mcp_servers.ragon_docs.args=[\"mcp\"]");
        for name in ["XDG_RUNTIME_DIR", "DBUS_SESSION_BUS_ADDRESS", "DREAMFERENCE_RAGON_AIRGAPPED", "https_proxy", "NO_PROXY"] {
            assert!(pdf[6].contains(&format!("\"{name}\"")), "{name}: {}", pdf[6]);
        }
        for setting in [&out[2], &out[4], &out[6]] {
            let (_, value) = setting.split_once('=').unwrap_or_default();
            assert!(format!("v = {value}").parse::<toml::Table>().is_ok(), "{setting}");
        }
        let own: Vec<OsString> = vec!["ragon".into(), "-c".into(), "mcp_servers.ragon_docs.enabled=false".into()];
        assert_eq!(with_tools(own.clone(), "# Your documents"), own);
        unsafe { std::env::remove_var("RAGON_DOCS_BIN") };
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_setting_reads_the_environment_then_the_file() {
        let dir = std::env::temp_dir().join(format!("ragon-docs-setting-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap_or_default();
        let file = dir.join("config.toml");
        std::fs::write(&file, "ragon_docs = false\n").unwrap_or_default();
        if std::env::var_os("DREAMFERENCE_RAGON_DOCS").is_none() {
            assert!(!enabled(Some(file.clone())));
            assert!(enabled(None));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
