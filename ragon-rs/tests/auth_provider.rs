//! The launcher's account-system half (specs/DREAMFERENCE_AUTH.md §5.1, §6, §11, §15), with no network.

use std::path::Path;

use ragon_launcher::auth::{self, pin, switcher};

#[test]
fn info_parses_and_an_older_answer_is_not_info() {
    let info = auth::parse_info(r#"{"node_id":"n1","node_name":"Box","primary_id":"p1","primary_name":"Ann"}"#).unwrap();
    assert_eq!((info.node_id.as_str(), info.primary_id.as_str(), info.primary_name.as_str()), ("n1", "p1", "Ann"));
    assert!(auth::parse_info("<html>").is_none());
    assert!(auth::parse_info(r#"{"models":[]}"#).is_none());
}

#[test]
fn the_label_is_the_host_without_scheme_or_port() {
    assert_eq!(auth::node_label("https://box.local:8000"), "box.local");
    assert_eq!(auth::node_label("http://127.0.0.1:8000/"), "127.0.0.1");
    assert_eq!(auth::node_label("http://[::1]:8000"), "::1");
}

#[test]
fn ragon_api_key_wins_over_openai_and_blank_is_unset() {
    assert_eq!(auth::api_key(Some("r"), Some("o")).as_deref(), Some("r"));
    assert_eq!(auth::api_key(None, Some("o")).as_deref(), Some("o"));
    assert_eq!(auth::api_key(Some("  "), Some("o")).as_deref(), Some("o"));
    assert_eq!(auth::api_key(None, None), None);
}

#[test]
fn the_environment_names_the_gate_the_ca_and_the_key() {
    let env = auth::codex_env("https://box:8000/", Some(Path::new("/h/ca.pem")), Some("k"), Some("o"));
    let get = |name: &str| env.iter().find(|(n, _)| *n == name).map(|(_, v)| v.as_str());
    assert_eq!(get("CODEX_APP_SERVER_LOGIN_ISSUER"), Some("https://box:8000"));
    assert_eq!(get("CODEX_REFRESH_TOKEN_URL_OVERRIDE"), Some("https://box:8000/oauth/token"));
    assert_eq!(get("CODEX_REVOKE_TOKEN_URL_OVERRIDE"), Some("https://box:8000/oauth/revoke"));
    assert_eq!(get("CODEX_CA_CERTIFICATE"), Some("/h/ca.pem"));
    assert_eq!(get("CODEX_API_KEY"), Some("k"));
    let bare = auth::codex_env("http://box:8000", None, None, None);
    assert!(bare.iter().all(|(n, _)| *n != "CODEX_CA_CERTIFICATE" && *n != "CODEX_API_KEY"));
    assert!(auth::user_agent_suffix().starts_with("ragon/"));
}

#[test]
fn a_ca_is_pinned_once_and_a_different_one_is_changed() {
    let dir = tempdir("pin");
    let pem = "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n";
    let first = pin::check_or_pin(&dir, "n1", pem).unwrap();
    let path = dir.join("nodes").join("n1").join("ca.pem");
    assert_eq!(first, pin::Outcome::Pinned(path.clone()));
    assert_eq!(pin::check_or_pin(&dir, "n1", &pem.replace('\n', "\r\n")).unwrap(), pin::Outcome::Known(path));
    let other = "-----BEGIN CERTIFICATE-----\nBBBB\n-----END CERTIFICATE-----\n";
    assert_eq!(pin::check_or_pin(&dir, "n1", other).unwrap(), pin::Outcome::Changed);
    assert!(pin::reset(&dir, "n1"));
    assert!(matches!(pin::check_or_pin(&dir, "n1", other).unwrap(), pin::Outcome::Pinned(_)));
}

#[test]
fn the_trust_store_commands_stay_in_the_users_own_store() {
    let home = Path::new("/home/u");
    let pem = Path::new("/home/u/.ragon/nodes/n1/ca.pem");
    // The stores are under `home`, joined with this platform's separator (Windows runs these too).
    let nss = format!("sql:{}", home.join(".pki").join("nssdb").display());
    let keychain = home.join("Library").join("Keychains").join("login.keychain-db").display().to_string();
    #[cfg(unix)]
    assert_eq!((nss.as_str(), keychain.as_str()), ("sql:/home/u/.pki/nssdb", "/home/u/Library/Keychains/login.keychain-db"));
    let linux = pin::trust_commands("linux", home, pem, "n1");
    assert_eq!(linux[0][0], "certutil");
    assert!(linux[0].contains(&nss));
    let mac = pin::trust_commands("macos", home, pem, "n1");
    assert!(mac[0].contains(&keychain));
    let win = pin::trust_commands("windows", home, pem, "n1");
    assert_eq!(&win[0][..4], ["certutil", "-user", "-addstore", "Root"]);
    assert!(pin::trust_commands("plan9", home, pem, "n1").is_empty());
}

#[test]
fn the_windows_acl_drops_inheritance() {
    let args = pin::windows_acl_args(Path::new("C:/u/auth.json"), "ann");
    assert_eq!(args, ["C:/u/auth.json", "/inheritance:r", "/grant:r", "ann:(F)"]);
}

#[test]
fn the_switcher_moves_only_the_marker() {
    let fresh = switcher::with_active(None, "p1").unwrap().unwrap();
    assert!(fresh.contains("\"active\": \"p1\"") && fresh.contains("\"version\": 2"));
    let two = r#"{"version":2,"active":"p1","entries":{"p1":{"t":1},"p2":{"t":2}}}"#;
    let moved = switcher::with_active(Some(two), "p2").unwrap().unwrap();
    let json: serde_json::Value = serde_json::from_str(&moved).unwrap();
    assert_eq!(json["active"], "p2");
    assert_eq!(json["entries"], serde_json::from_str::<serde_json::Value>(two).unwrap()["entries"]);
    assert!(switcher::with_active(Some(two), "p1").unwrap().is_none());
    assert!(switcher::with_active(Some(r#"{"tokens":{}}"#), "p1").unwrap().is_none(), "the old layout is Codex's");
    assert!(switcher::with_active(Some("nope"), "p1").is_err());
}

#[test]
fn set_active_writes_the_file_under_a_lock_and_leaves_no_lock() {
    let dir = tempdir("sw");
    switcher::set_active(&dir, "p1").unwrap();
    switcher::set_active(&dir, "p2").unwrap();
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("auth.json")).unwrap()).unwrap();
    assert_eq!(json["active"], "p2");
    assert!(!dir.join("auth.json.lock").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(dir.join("auth.json")).unwrap().permissions().mode() & 0o777, 0o600);
    }
}

#[tokio::test]
async fn a_plain_http_node_is_served_as_before() {
    let dir = tempdir("start");
    auth::start("http://127.0.0.1:9", &dir).await.unwrap();
    assert!(!dir.join("auth.json").exists());
    assert!(!dir.join("nodes").exists());
}

fn tempdir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ragon-auth-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
