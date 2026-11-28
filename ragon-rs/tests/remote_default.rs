//! A bare `ragon --remote` dials the primary with the signed-in account; an explicit address is
//! Codex's (specs/DREAMFERENCE_RAGON_REMOTE_SESSIONS.md §7.1 A5, R1, D12). A fake node config and
//! a fake account store in a temporary folder; no sign-in, no network.

use std::ffi::OsString;
use std::path::Path;

use clap::{Arg, Command};
use ragon_launcher::{
    REMOTE_TOKEN_ENV, RemoteDefault, bare_remote_index, gate_node_file, no_primary_line, primary_name_from_node_file,
    primary_remote_url, remote_default,
};

const BEARER: &str = "test-access-token-not-real";

/// Codex's `--remote` options as `InteractiveRemoteOptions` declares them, on the root and on a
/// subcommand.
fn codex_like() -> Command {
    let remote = || {
        [
            Arg::new("remote").long("remote").value_name("ADDR"),
            Arg::new("remote-auth-token-env").long("remote-auth-token-env").value_name("ENV_VAR"),
        ]
    };
    let mut command = Command::new("ragon")
        .args(remote())
        .arg(Arg::new("prompt"))
        .subcommand(Command::new("resume").args(remote()).arg(Arg::new("session")))
        .subcommand(Command::new("exec").arg(Arg::new("prompt")));
    command.build();
    command
}

/// A folder of its own under the system's temp folder, removed when dropped.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("ragon-remote-default-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn args(words: &[&str]) -> Vec<OsString> {
    std::iter::once("ragon").chain(words.iter().copied()).map(OsString::from).collect()
}

fn strings(args: &[OsString]) -> Vec<String> {
    args.iter().map(|a| a.to_string_lossy().into_owned()).collect()
}

/// A home whose node names `primary` as its primary, and a Ragon home whose active entry holds
/// `bearer`.
fn fixture(primary: Option<&str>, bearer: Option<&str>) -> (Scratch, std::path::PathBuf, std::path::PathBuf) {
    let tmp = Scratch::new();
    let home = tmp.path().join("home");
    let node = gate_node_file(&home);
    std::fs::create_dir_all(node.parent().unwrap()).unwrap();
    let doc = serde_json::json!({
        "node_id": "n-1", "name": primary.unwrap_or("second"), "primary_id": primary.map(|_| "n-1"),
        "primary_name": primary, "accounts": true,
    });
    std::fs::write(&node, doc.to_string()).unwrap();
    let codex_home = home.join(".ragon");
    std::fs::create_dir_all(&codex_home).unwrap();
    if let Some(bearer) = bearer {
        let auth = serde_json::json!({
            "version": 2, "active": "n-1",
            "entries": { "n-1": { "tokens": { "access_token": bearer, "refresh_token": "r", "id_token": "i" } } },
        });
        std::fs::write(codex_home.join("auth.json"), auth.to_string()).unwrap();
    }
    (tmp, home, codex_home)
}

fn listing(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for entry in walk(dir) {
        let bytes = std::fs::read(&entry).unwrap_or_default();
        out.push((entry.to_string_lossy().into_owned(), bytes));
    }
    out.sort();
    out
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() { files.extend(walk(&path)) } else { files.push(path) }
    }
    files
}

#[test]
fn bare_remote_dials_the_primary_with_the_account_token() {
    let (_tmp, home, codex_home) = fixture(Some("dgx-a"), Some(BEARER));
    let got = remote_default(&codex_like(), &args(&["--remote"]), &home, &codex_home, None).unwrap().unwrap();
    assert_eq!(
        strings(&got.args),
        ["ragon", "--remote", "wss://dgx-a.local:8000/", "--remote-auth-token-env", REMOTE_TOKEN_ENV]
    );
    assert_eq!(got.token.as_deref(), Some(BEARER));
}

#[test]
fn the_filled_line_parses_as_codex_reads_it() {
    let (_tmp, home, codex_home) = fixture(Some("dgx-a"), Some(BEARER));
    let line = args(&["--remote", "fix the tests"]);
    // "fix the tests" is an address to Codex: only the forms Codex refuses are bare.
    assert_eq!(remote_default(&codex_like(), &line, &home, &codex_home, None).unwrap(), None);
    let line = args(&["resume", "--remote"]);
    let got = remote_default(&codex_like(), &line, &home, &codex_home, None).unwrap().unwrap();
    let matches = codex_like().try_get_matches_from(&got.args).unwrap();
    let (name, sub) = matches.subcommand().unwrap();
    assert_eq!(name, "resume");
    assert_eq!(sub.get_one::<String>("remote").map(String::as_str), Some("wss://dgx-a.local:8000/"));
    assert_eq!(sub.get_one::<String>("remote-auth-token-env").map(String::as_str), Some(REMOTE_TOKEN_ENV));
}

#[test]
fn a_bare_remote_before_another_flag_or_a_subcommand_is_bare() {
    let command = codex_like();
    assert_eq!(bare_remote_index(&command, &args(&["--remote"])), Some(1));
    assert_eq!(bare_remote_index(&command, &args(&["--remote", "-m", "x"])), Some(1));
    assert_eq!(bare_remote_index(&command, &args(&["--remote", "resume", "--last"])), Some(1));
    assert_eq!(bare_remote_index(&command, &args(&["resume", "--remote"])), Some(2));
    assert_eq!(bare_remote_index(&command, &args(&["--", "--remote"])), None);
    let (_tmp, home, codex_home) = fixture(Some("dgx-a"), Some(BEARER));
    let got = remote_default(&command, &args(&["--remote", "resume", "--last"]), &home, &codex_home, None).unwrap().unwrap();
    assert_eq!(
        strings(&got.args),
        ["ragon", "--remote", "wss://dgx-a.local:8000/", "--remote-auth-token-env", REMOTE_TOKEN_ENV, "resume", "--last"]
    );
}

#[test]
fn an_explicit_address_is_left_to_codex() {
    let command = codex_like();
    let (_tmp, home, codex_home) = fixture(Some("dgx-a"), Some(BEARER));
    for line in [
        vec!["--remote", "wss://other.local:8000/"],
        vec!["--remote=wss://other.local:8000/"],
        vec!["--remote", "ws://127.0.0.1:4500"],
        // Off loopback with a token: Codex's own check refuses it, unchanged.
        vec!["--remote", "ws://other.lan:4500", "--remote-auth-token-env", "MY_TOKEN"],
        vec!["--remote", "unix://"],
        // Without a port or with a path: Codex's value check, not the launcher's.
        vec!["--remote", "wss://other.local/"],
        vec!["--remote=", "-m", "x"],
        vec!["exec", "hello"],
    ] {
        assert_eq!(bare_remote_index(&command, &args(&line)), None, "{line:?}");
        assert_eq!(remote_default(&command, &args(&line), &home, &codex_home, None).unwrap(), None, "{line:?}");
    }
}

#[test]
fn a_token_variable_the_user_named_is_kept() {
    let (_tmp, home, codex_home) = fixture(Some("dgx-a"), Some(BEARER));
    let line = args(&["--remote-auth-token-env", "MY_TOKEN", "--remote"]);
    let got = remote_default(&codex_like(), &line, &home, &codex_home, None).unwrap().unwrap();
    assert_eq!(
        got,
        RemoteDefault {
            args: args(&["--remote-auth-token-env", "MY_TOKEN", "--remote", "wss://dgx-a.local:8000/"]),
            token: None,
        }
    );
}

#[test]
fn an_api_key_counts_as_the_signed_in_account_and_no_account_dials_without_a_token() {
    let (_tmp, home, codex_home) = fixture(Some("dgx-a"), None);
    let got = remote_default(&codex_like(), &args(&["--remote"]), &home, &codex_home, Some("rk-test")).unwrap().unwrap();
    assert_eq!(got.token.as_deref(), Some("rk-test"));
    let got = remote_default(&codex_like(), &args(&["--remote"]), &home, &codex_home, None).unwrap().unwrap();
    assert_eq!(strings(&got.args), ["ragon", "--remote", "wss://dgx-a.local:8000/"]);
    assert_eq!(got.token, None);
}

#[test]
fn a_node_that_names_no_primary_refuses_with_the_explicit_form() {
    for primary in [None, Some("bad name"), Some("")] {
        let (_tmp, home, codex_home) = fixture(primary, Some(BEARER));
        let err = remote_default(&codex_like(), &args(&["--remote"]), &home, &codex_home, None).unwrap_err();
        assert_eq!(err, no_primary_line());
        assert!(!err.contains(BEARER));
    }
    let tmp = Scratch::new();
    assert!(remote_default(&codex_like(), &args(&["--remote"]), tmp.path(), tmp.path(), None).is_err());
}

#[test]
fn the_token_is_never_written_and_never_in_the_arguments() {
    let (tmp, home, codex_home) = fixture(Some("dgx-a"), Some(BEARER));
    let before = listing(tmp.path());
    let got = remote_default(&codex_like(), &args(&["--remote"]), &home, &codex_home, None).unwrap().unwrap();
    assert_eq!(listing(tmp.path()), before);
    assert!(strings(&got.args).iter().all(|arg| !arg.contains(BEARER)));
    assert!(!format!("{got:?}").contains(BEARER));
}

#[test]
fn the_node_file_and_the_url() {
    assert_eq!(primary_name_from_node_file(r#"{"primary_name":"gx10-1a2b"}"#).as_deref(), Some("gx10-1a2b"));
    assert_eq!(primary_name_from_node_file(r#"{"primary_name":null}"#), None);
    assert_eq!(primary_name_from_node_file(r#"{"primary_name":"a.b"}"#), None);
    assert_eq!(primary_name_from_node_file("not json"), None);
    assert_eq!(primary_remote_url("gx10-1a2b"), "wss://gx10-1a2b.local:8000/");
}
