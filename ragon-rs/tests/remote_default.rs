//! A bare `ragon --remote` dials the primary with the signed-in account; an explicit address is
//! Codex's (specs/DREAMFERENCE_RAGON_REMOTE_SESSIONS.md §7.1 A5, R1, D12). A fake node config and
//! a fake account store in a temporary folder; no sign-in, no network.

use std::ffi::OsString;
use std::path::Path;

use clap::{Arg, Command};
use ragon_launcher::{
    REMOTE_TOKEN_ENV, RemoteDefault, bare_remote_index, gate_node_file, no_primary_line, primary_from_node_file,
    primary_name_from_id_token, primary_remote_url, remote_default,
};
use serde_json::{Value, json};

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
    // `sandbox` and `mcp add` take the rest of the line as the command they run, as Codex's do.
    let trailing = |name: &'static str| Arg::new(name).num_args(1..).trailing_var_arg(true).allow_hyphen_values(true);
    let mut command = Command::new("ragon")
        .arg(Arg::new("config").short('c').long("config").global(true))
        .args(remote())
        .arg(Arg::new("prompt"))
        .subcommand(Command::new("resume").args(remote()).arg(Arg::new("session")))
        .subcommand(Command::new("fork").args(remote()).arg(Arg::new("session")))
        .subcommand(Command::new("exec").arg(Arg::new("prompt")))
        .subcommand(Command::new("sandbox").subcommand(Command::new("linux").arg(trailing("command"))))
        .subcommand(Command::new("mcp").subcommand(Command::new("add").arg(Arg::new("name")).arg(trailing("command"))));
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

/// An id token as the primary `name` signs it: its issuer is the primary's front door.
fn id_token(name: &str) -> String {
    use base64::Engine;
    let part = |value: Value| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(value.to_string());
    format!("{}.{}.sig", part(json!({ "alg": "RS256" })), part(json!({ "iss": format!("https://{name}.local:8000"), "sub": "u-1" })))
}

/// An account entry of the primary `name`, as Codex stores a sign-in.
fn token_entry(name: &str, bearer: &str) -> Value {
    json!({ "tokens": { "access_token": bearer, "refresh_token": "r", "id_token": id_token(name) } })
}

/// A machine: its gate's `node.json` (none on a client), and `auth.json` with `active` and its
/// entries (no file when `active` is `None`).
fn machine(node: Option<Value>, active: Option<&str>, entries: Value) -> (Scratch, std::path::PathBuf, std::path::PathBuf) {
    let tmp = Scratch::new();
    let home = tmp.path().join("home");
    let codex_home = home.join(".ragon");
    std::fs::create_dir_all(&codex_home).unwrap();
    if let Some(doc) = node {
        let file = gate_node_file(&home);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, doc.to_string()).unwrap();
    }
    if let Some(active) = active {
        let auth = json!({ "version": 2, "active": active, "entries": entries });
        std::fs::write(codex_home.join("auth.json"), auth.to_string()).unwrap();
    }
    (tmp, home, codex_home)
}

/// `node.json` of the primary `name` (id `n-1`).
fn primary_node(name: &str) -> Value {
    json!({ "node_id": "n-1", "name": name, "primary_id": "n-1", "primary_name": name, "accounts": true })
}

/// `node.json` of a secondary: it names no primary.
fn secondary_node() -> Value {
    json!({ "node_id": "s-1", "name": "second", "primary_id": null, "primary_name": null, "accounts": true })
}

/// The primary `primary` itself (id `n-1`), signed in to its own account when `bearer` is given.
/// With no `primary`, a secondary signed in to an entry whose token names no primary.
fn fixture(primary: Option<&str>, bearer: Option<&str>) -> (Scratch, std::path::PathBuf, std::path::PathBuf) {
    let node = match primary {
        Some(name) => primary_node(name),
        None => secondary_node(),
    };
    let entry = match (primary, bearer) {
        (Some(name), Some(bearer)) => token_entry(name, bearer),
        (None, Some(bearer)) => json!({ "tokens": { "access_token": bearer, "refresh_token": "r", "id_token": "i" } }),
        (_, None) => return machine(Some(node), None, json!({})),
    };
    machine(Some(node), Some("n-1"), json!({ "n-1": entry }))
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
    let got = remote_default(&codex_like(), &args(&["--remote"]), &home, &codex_home).unwrap().unwrap();
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
    assert_eq!(remote_default(&codex_like(), &line, &home, &codex_home).unwrap(), None);
    let line = args(&["resume", "--remote"]);
    let got = remote_default(&codex_like(), &line, &home, &codex_home).unwrap().unwrap();
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
    // An option's value is skipped, and Codex's root options may follow the prompt.
    assert_eq!(bare_remote_index(&command, &args(&["-c", "--remote", "--remote"])), Some(3));
    assert_eq!(bare_remote_index(&command, &args(&["fix the tests", "--remote"])), Some(2));
    let (_tmp, home, codex_home) = fixture(Some("dgx-a"), Some(BEARER));
    let got = remote_default(&command, &args(&["--remote", "resume", "--last"]), &home, &codex_home).unwrap().unwrap();
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
        assert_eq!(remote_default(&command, &args(&line), &home, &codex_home).unwrap(), None, "{line:?}");
    }
}

#[test]
fn a_token_variable_the_user_named_is_kept() {
    let (_tmp, home, codex_home) = fixture(Some("dgx-a"), Some(BEARER));
    let line = args(&["--remote-auth-token-env", "MY_TOKEN", "--remote"]);
    let got = remote_default(&codex_like(), &line, &home, &codex_home).unwrap().unwrap();
    assert_eq!(
        got,
        RemoteDefault {
            args: args(&["--remote-auth-token-env", "MY_TOKEN", "--remote", "wss://dgx-a.local:8000/"]),
            token: None,
        }
    );
}

#[test]
fn the_primary_itself_dials_itself_with_a_key_entry_or_without_an_account() {
    // A key entry names no issuer: the node's own file names the primary, when its id is the active one.
    let (_tmp, home, codex_home) = machine(Some(primary_node("dgx-a")), Some("n-1"), json!({ "n-1": { "OPENAI_API_KEY": "rk-test" } }));
    let got = remote_default(&codex_like(), &args(&["--remote"]), &home, &codex_home).unwrap().unwrap();
    assert_eq!(strings(&got.args)[2], "wss://dgx-a.local:8000/");
    assert_eq!(got.token.as_deref(), Some("rk-test"));
    // Not signed in: the dial goes without a token.
    let (_tmp, home, codex_home) = fixture(Some("dgx-a"), None);
    let got = remote_default(&codex_like(), &args(&["--remote"]), &home, &codex_home).unwrap().unwrap();
    assert_eq!(strings(&got.args), ["ragon", "--remote", "wss://dgx-a.local:8000/"]);
    assert_eq!(got.token, None);
}

#[test]
fn a_client_with_no_node_dials_the_signed_in_primary() {
    let (_tmp, home, codex_home) = machine(None, Some("p-1"), json!({ "p-1": token_entry("dgx-a", BEARER) }));
    let got = remote_default(&codex_like(), &args(&["--remote"]), &home, &codex_home).unwrap().unwrap();
    assert_eq!(
        strings(&got.args),
        ["ragon", "--remote", "wss://dgx-a.local:8000/", "--remote-auth-token-env", REMOTE_TOKEN_ENV]
    );
    assert_eq!(got.token.as_deref(), Some(BEARER));
    // Not signed in, and no node to fall back on.
    let (_tmp, home, codex_home) = machine(None, None, json!({}));
    assert_eq!(remote_default(&codex_like(), &args(&["--remote"]), &home, &codex_home).unwrap_err(), no_primary_line());
}

#[test]
fn a_secondary_whose_node_names_no_primary_dials_the_signed_in_primary() {
    let (_tmp, home, codex_home) = machine(Some(secondary_node()), Some("p-1"), json!({ "p-1": token_entry("dgx-a", BEARER) }));
    let got = remote_default(&codex_like(), &args(&["--remote"]), &home, &codex_home).unwrap().unwrap();
    assert_eq!(strings(&got.args)[2], "wss://dgx-a.local:8000/");
    assert_eq!(got.token.as_deref(), Some(BEARER));
}

#[test]
fn the_token_is_the_dialed_primarys_own() {
    // Two primaries in the switcher, the second active, on a machine that is the first primary.
    let entries = json!({ "n-1": token_entry("dgx-a", "token-of-a"), "p-2": token_entry("dgx-b", "token-of-b") });
    let (_tmp, home, codex_home) = machine(Some(primary_node("dgx-a")), Some("p-2"), entries);
    let got = remote_default(&codex_like(), &args(&["--remote"]), &home, &codex_home).unwrap().unwrap();
    assert_eq!(strings(&got.args)[2], "wss://dgx-b.local:8000/");
    assert_eq!(got.token.as_deref(), Some("token-of-b"));
    // A key entry of another primary is never sent to the primary this node names.
    let (_tmp, home, codex_home) = machine(Some(primary_node("dgx-a")), Some("p-2"), json!({ "p-2": { "OPENAI_API_KEY": "key-of-b" } }));
    let err = remote_default(&codex_like(), &args(&["--remote"]), &home, &codex_home).unwrap_err();
    assert_eq!(err, no_primary_line());
}

#[test]
fn a_node_that_names_no_primary_refuses_with_the_explicit_form() {
    for primary in [None, Some("bad name"), Some("")] {
        let (_tmp, home, codex_home) = fixture(primary, Some(BEARER));
        let err = remote_default(&codex_like(), &args(&["--remote"]), &home, &codex_home).unwrap_err();
        assert_eq!(err, no_primary_line());
        assert!(!err.contains(BEARER));
    }
    let tmp = Scratch::new();
    assert!(remote_default(&codex_like(), &args(&["--remote"]), tmp.path(), tmp.path()).is_err());
}

#[test]
fn the_token_is_never_written_and_never_in_the_arguments() {
    let (tmp, home, codex_home) = fixture(Some("dgx-a"), Some(BEARER));
    let before = listing(tmp.path());
    let got = remote_default(&codex_like(), &args(&["--remote"]), &home, &codex_home).unwrap().unwrap();
    assert_eq!(listing(tmp.path()), before);
    assert!(strings(&got.args).iter().all(|arg| !arg.contains(BEARER)));
    assert!(!format!("{got:?}").contains(BEARER));
}

#[test]
fn the_node_file_and_the_url() {
    assert_eq!(
        primary_from_node_file(r#"{"primary_id":"n-1","primary_name":"gx10-1a2b"}"#),
        Some(("n-1".to_string(), "gx10-1a2b".to_string()))
    );
    assert_eq!(primary_from_node_file(r#"{"primary_id":null,"primary_name":null}"#), None);
    assert_eq!(primary_from_node_file(r#"{"primary_id":"n-1","primary_name":"a.b"}"#), None);
    assert_eq!(primary_from_node_file("not json"), None);
    assert_eq!(primary_name_from_id_token(&id_token("gx10-1a2b")).as_deref(), Some("gx10-1a2b"));
    assert_eq!(primary_name_from_id_token("i"), None);
    assert_eq!(primary_name_from_id_token(&id_token("a.b")), None);
    assert_eq!(primary_remote_url("gx10-1a2b"), "wss://gx10-1a2b.local:8000/");
}

#[test]
fn a_remote_in_a_command_codex_runs_is_never_touched() {
    let command = codex_like();
    let (_tmp, home, codex_home) = fixture(Some("dgx-a"), Some(BEARER));
    for line in [
        vec!["sandbox", "linux", "python", "app.py", "--remote"],
        vec!["sandbox", "linux", "--remote"],
        vec!["mcp", "add", "srv", "npx", "pkg", "--remote"],
        vec!["-c", "x=1", "mcp", "add", "srv", "--", "pkg", "--remote"],
        // `exec` takes no `--remote`: Codex's own error, not a dial.
        vec!["exec", "--remote"],
    ] {
        assert_eq!(bare_remote_index(&command, &args(&line)), None, "{line:?}");
        assert_eq!(remote_default(&command, &args(&line), &home, &codex_home).unwrap(), None, "{line:?}");
    }
}

#[test]
fn a_subcommand_name_after_a_subcommand_is_looked_up_in_that_subcommand() {
    let command = codex_like();
    // `fork` is a root subcommand, not one of `resume`'s: Codex reads it as the address.
    assert_eq!(bare_remote_index(&command, &args(&["resume", "--remote", "fork"])), None);
    // At the root it is the subcommand, so the `--remote` before it is bare.
    assert_eq!(bare_remote_index(&command, &args(&["--remote", "fork"])), Some(1));
    assert_eq!(bare_remote_index(&command, &args(&["fork", "--remote"])), Some(2));
}

#[test]
fn a_token_variable_after_the_double_dash_is_not_the_users() {
    let (_tmp, home, codex_home) = fixture(Some("dgx-a"), Some(BEARER));
    let line = args(&["--remote", "--", "--remote-auth-token-env", "MY_TOKEN"]);
    let got = remote_default(&codex_like(), &line, &home, &codex_home).unwrap().unwrap();
    assert_eq!(got.token.as_deref(), Some(BEARER));
    assert_eq!(
        strings(&got.args),
        ["ragon", "--remote", "wss://dgx-a.local:8000/", "--remote-auth-token-env", REMOTE_TOKEN_ENV, "--", "--remote-auth-token-env", "MY_TOKEN"]
    );
}
