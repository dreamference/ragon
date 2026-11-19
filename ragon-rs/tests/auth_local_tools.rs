//! The local tools read the bearer from the active entry of auth.json (AUTH §14.5).

use ragon_launcher::auth::{bearer, bearer_from_text};

const V2: &str = r#"{"version":2,"active":"b","entries":{
  "a":{"auth_mode":"chatgpt","tokens":{"access_token":"tok-a"}},
  "b":{"auth_mode":"chatgpt","tokens":{"access_token":"tok-b"}}}}"#;

#[test]
fn the_active_entry_of_a_version_2_file_is_the_bearer() {
    assert_eq!(bearer_from_text(V2).as_deref(), Some("tok-b"));
}

#[test]
fn a_one_entry_file_and_an_api_key_entry_are_read() {
    assert_eq!(bearer_from_text(r#"{"tokens":{"access_token":"solo"}}"#).as_deref(), Some("solo"));
    assert_eq!(bearer_from_text(r#"{"OPENAI_API_KEY":"sk-ragon-1"}"#).as_deref(), Some("sk-ragon-1"));
}

#[test]
fn no_active_entry_or_no_credential_gives_none() {
    assert_eq!(bearer_from_text(r#"{"version":2,"active":"x","entries":{"a":{"tokens":{"access_token":"t"}}}}"#), None);
    assert_eq!(bearer_from_text("{}"), None);
    assert_eq!(bearer_from_text("not json"), None);
}

#[test]
fn the_environment_key_wins_and_the_file_is_the_fallback() {
    let dir = std::env::temp_dir().join(format!("ragon-auth-local-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("auth.json"), V2).unwrap();
    assert_eq!(bearer(&dir, Some("sk-ragon-env")).as_deref(), Some("sk-ragon-env"));
    assert_eq!(bearer(&dir, Some("  ")).as_deref(), Some("tok-b"));
    assert_eq!(bearer(&dir, None).as_deref(), Some("tok-b"));
    assert_eq!(bearer(&dir.join("missing"), None), None);
    let _ = std::fs::remove_dir_all(&dir);
}
