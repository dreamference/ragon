use base64::Engine;
use ragon_launcher::auth::slash::{self, Plan};

fn token(claims: &serde_json::Value) -> String {
    let enc = |v: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v);
    format!("{}.{}.sig", enc(b"{}"), enc(claims.to_string().as_bytes()))
}

fn auth_file(role: &str) -> String {
    let id = token(&serde_json::json!({
        "email": "ann",
        "https://api.openai.com/auth": {"chatgpt_plan_type": role},
        "https://ragon.dev/auth": {"primary_name": "Box"},
    }));
    serde_json::json!({"version": 2, "active": "p1", "entries": {
        "p1": {"tokens": {"id_token": id, "access_token": "tok"}},
        "p2": {"tokens": {"id_token": token(&serde_json::json!({"email": "bob"})), "access_token": "x"}},
    }})
    .to_string()
}

#[test]
fn the_account_is_the_active_entry() {
    let account = slash::account_from_auth(&auth_file("admin")).unwrap();
    assert_eq!((account.user.as_str(), account.role.as_str(), account.node.as_deref()), ("ann", "admin", Some("Box")));
    assert_eq!(account.bearer.as_deref(), Some("tok"));
    assert_eq!(slash::item(Some(&account)).as_deref(), Some("ann @ Box"));
    assert_eq!(slash::whoami(Some(&account), false), "Signed in as ann (admin) at Box");
}

#[test]
fn no_entry_means_no_item_and_not_signed_in() {
    assert!(slash::account_from_auth(r#"{"version":2,"active":"p1","entries":{}}"#).is_none());
    assert!(slash::account_from_auth("not json").is_none());
    assert_eq!(slash::item(None), None);
    assert_eq!(slash::whoami(None, false), "Not signed in. Run ragon login.");
    assert!(slash::whoami(None, true).starts_with("Using API key"));
}

fn asked(cmd: &str, args: &str, user: Option<&str>) -> (String, String, Option<serde_json::Value>) {
    match slash::plan(cmd, args, user) {
        Plan::Ask(r) => (r.method.to_string(), r.path, r.body),
        Plan::Say(lines) => panic!("expected a request, got {lines:?}"),
    }
}

#[test]
fn every_command_maps_to_its_endpoint() {
    assert_eq!(asked("password", "old new", None), ("POST".into(), "/ragon/v1/users/me/password".into(), Some(serde_json::json!({"current": "old", "new": "new"}))));
    assert_eq!(asked("apikey", "", None).1, "/ragon/v1/users/me/keys");
    assert_eq!(asked("apikey", "new ci", None).2, Some(serde_json::json!({"name": "ci"})));
    assert_eq!(asked("apikey", "revoke ci", None), ("DELETE".into(), "/ragon/v1/users/me/keys/ci".into(), None));
    assert_eq!(asked("sessions", "revoke --all", None).1, "/ragon/v1/users/me/sessions");
    assert_eq!(asked("sessions", "revoke s1", None).1, "/ragon/v1/users/me/sessions/s1");
    assert_eq!(asked("users", "", None).1, "/ragon/v1/users");
    assert_eq!(asked("users", "promote bob", None), ("POST".into(), "/ragon/v1/users/bob/promote".into(), None));
    assert_eq!(asked("users", "remove bob", None).0, "DELETE");
}

#[test]
fn bad_arguments_get_a_usage_line_and_never_a_request() {
    for (cmd, args) in [("password", "only-one"), ("apikey", "revoke"), ("sessions", "x"), ("users", "promote"), ("users", "remove ../x"), ("whoami", "")] {
        assert!(matches!(slash::plan(cmd, args, Some("ann")), Plan::Say(_)), "{cmd} {args}");
    }
}

#[test]
fn delete_account_needs_the_own_username() {
    assert!(matches!(slash::plan("delete-account", "bob", Some("ann")), Plan::Say(_)));
    assert!(matches!(slash::plan("delete-account", "", Some("ann")), Plan::Say(_)));
    assert!(matches!(slash::plan("delete-account", "ann", None), Plan::Say(_)));
    assert_eq!(asked("delete-account", "ann", Some("ann")), ("DELETE".into(), "/ragon/v1/users/me".into(), None));
}

#[test]
fn answers_read_as_lines() {
    assert_eq!(slash::render("users", 403, "{}"), vec!["/users needs the admin role."]);
    assert!(slash::render("apikey", 401, "").pop().unwrap().contains("ragon login"));
    assert_eq!(slash::render("apikey", 200, r#"{"key":"sk-ragon-abc"}"#), vec!["New API key (shown once): sk-ragon-abc"]);
    assert_eq!(slash::render("sessions", 200, "[]"), vec!["Nothing to show."]);
    assert_eq!(slash::render("users", 200, r#"[{"name":"ann","role":"admin"}]"#), vec!["name=ann  role=admin"]);
    assert_eq!(slash::render("password", 400, r#"{"error":{"message":"wrong password"}}"#), vec!["/password: wrong password"]);
    assert_eq!(slash::render("delete-account", 204, ""), vec!["Done."]);
}
