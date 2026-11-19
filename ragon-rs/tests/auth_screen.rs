//! The sign-in step's screen (specs/DREAMFERENCE_AUTH.md §8): keys, forms, the grant and the token
//! answer, with no network and no terminal.

use base64::Engine as _;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ragon_launcher::auth::screen::{self, Grant, Kind, Node, Outcome, Screen, Step, View};

fn node(primary: &str, upgrade: bool) -> Node {
    Node {
        gate: "https://box.local:8000".to_string(),
        node_name: "box".to_string(),
        primary_name: primary.to_string(),
        upgrade,
        ..Node::default()
    }
}

fn press(screen: &mut Screen, code: KeyCode) -> Step {
    screen.key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn typed(screen: &mut Screen, text: &str) {
    for c in text.chars() {
        assert_eq!(press(screen, KeyCode::Char(c)), Step::Redraw);
    }
}

fn text(screen: &Screen) -> String {
    screen
        .lines()
        .iter()
        .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn jwt(claims: serde_json::Value) -> String {
    let encode = |value: &serde_json::Value| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(value.to_string());
    format!("{}.{}.sig", encode(&serde_json::json!({"alg": "RS256", "typ": "JWT"})), encode(&claims))
}

fn tokens_for(role: &str) -> String {
    let id = jwt(serde_json::json!({
        "sub": "u1",
        "email": "t-alice",
        "https://api.openai.com/auth": {"chatgpt_plan_type": role, "chatgpt_user_id": "u1", "chatgpt_account_id": "u1"},
    }));
    serde_json::json!({"id_token": id, "access_token": id, "refresh_token": "r1", "expires_in": 3600, "token_type": "Bearer"}).to_string()
}

#[test]
fn the_picker_keeps_codexs_four_lines_and_keys() {
    let mut screen = Screen::new(&node("prime", false));
    assert_eq!(screen.view, View::Pick);
    let shown = text(&screen);
    assert!(shown.contains("Sign in to box (accounts kept by prime)"), "{shown}");
    for line in [
        "1. Sign in to prime",
        "2. Create an account on prime",
        "3. Sign in with a device code",
        "Sign in from another device with a one-time code",
        "4. Provide your own API key",
        "Paste an API key minted with /apikey or by an admin",
        "Press Enter to continue",
    ] {
        assert!(shown.contains(line), "{line} missing from {shown}");
    }
    assert!(!shown.contains("ChatGPT") && !shown.contains("OpenAI"));
    assert!(!screen.typing());
    assert_eq!(press(&mut screen, KeyCode::Down), Step::Redraw);
    assert_eq!(press(&mut screen, KeyCode::Char('j')), Step::Redraw);
    assert_eq!(screen.highlight, 2);
    assert_eq!(press(&mut screen, KeyCode::Enter), Step::DeviceCode);
    assert_eq!(press(&mut screen, KeyCode::Char('4')), Step::ApiKey);
    assert_eq!(press(&mut screen, KeyCode::Char('k')), Step::Redraw);
    assert_eq!(screen.highlight, 2);
    // Codex's quit chords stay Codex's.
    assert_eq!(screen.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)), Step::Pass);
    // A primary's own screen has no "kept by" part.
    assert!(text(&Screen::new(&node("box", false))).starts_with("  Sign in to box\n"));
}

#[test]
fn an_empty_or_blank_password_is_sent_as_typed() {
    let mut screen = Screen::new(&node("prime", false));
    assert_eq!(press(&mut screen, KeyCode::Char('1')), Step::Redraw);
    assert!(screen.typing(), "a form makes printable keys text");
    typed(&mut screen, "t-empty");
    assert_eq!(press(&mut screen, KeyCode::Enter), Step::Redraw);
    let step = press(&mut screen, KeyCode::Enter);
    assert_eq!(step, Step::Send(Grant { username: "t-empty".to_string(), ..Grant::default() }));
    assert!(matches!(screen.view, View::Working(_)));
    assert!(!screen.typing());

    let mut screen = Screen::new(&node("prime", false));
    press(&mut screen, KeyCode::Char('y'));
    typed(&mut screen, "t-alice");
    press(&mut screen, KeyCode::Tab);
    typed(&mut screen, " q y ");
    assert!(text(&screen).contains("Password: •••••"), "the password is masked");
    assert!(!text(&screen).contains(" q y "));
    let Step::Send(grant) = press(&mut screen, KeyCode::Enter) else { panic!("not sent") };
    assert_eq!(grant.password, " q y ");
    assert!(!grant.create);
}

#[test]
fn a_paste_lands_in_the_focused_field_untrimmed() {
    let mut screen = Screen::new(&node("prime", false));
    assert!(!screen.paste("x"), "the picker takes no paste");
    press(&mut screen, KeyCode::Char('1'));
    assert!(screen.paste("t-alice"));
    press(&mut screen, KeyCode::Enter);
    assert!(screen.paste("  secret \n"));
    let Step::Send(grant) = press(&mut screen, KeyCode::Enter) else { panic!("not sent") };
    assert_eq!((grant.username.as_str(), grant.password.as_str()), ("t-alice", "  secret "));
}

#[test]
fn the_create_form_checks_the_name_the_match_and_asks_once_about_an_empty_password() {
    let mut screen = Screen::new(&node("prime", false));
    press(&mut screen, KeyCode::Char('2'));
    typed(&mut screen, "bad name");
    for _ in 0..3 {
        press(&mut screen, KeyCode::Enter);
    }
    assert_eq!(screen.error.as_deref(), Some("Usernames are 1-32 letters, digits, '.', '_' or '-'."));
    for _ in 0.."bad name".len() {
        press(&mut screen, KeyCode::Backspace);
    }
    typed(&mut screen, "t-bob");
    press(&mut screen, KeyCode::Tab);
    typed(&mut screen, "a");
    press(&mut screen, KeyCode::Tab);
    typed(&mut screen, "b");
    assert_eq!(press(&mut screen, KeyCode::Enter), Step::Redraw);
    assert_eq!(screen.error.as_deref(), Some("The passwords do not match."));
    press(&mut screen, KeyCode::Backspace);
    press(&mut screen, KeyCode::Tab);
    press(&mut screen, KeyCode::Backspace);
    press(&mut screen, KeyCode::Enter);
    assert!(matches!(screen.view, View::AskEmpty(_)));
    assert!(text(&screen).contains("Continue without a password?"));
    assert_eq!(press(&mut screen, KeyCode::Char('n')), Step::Redraw);
    assert!(matches!(screen.view, View::Form(_)));
    press(&mut screen, KeyCode::Enter);
    let step = press(&mut screen, KeyCode::Char('y'));
    assert_eq!(step, Step::Send(Grant { username: "t-bob".to_string(), create: true, ..Grant::default() }));
}

#[test]
fn esc_goes_back_to_the_picker() {
    let mut screen = Screen::new(&node("prime", false));
    press(&mut screen, KeyCode::Char('2'));
    typed(&mut screen, "x");
    assert_eq!(press(&mut screen, KeyCode::Esc), Step::Redraw);
    assert_eq!(screen.view, View::Pick);
}

#[test]
fn the_token_answer_is_read_as_the_primary_writes_it() {
    let tokens = screen::classify(200, &tokens_for("user")).unwrap();
    assert_eq!(tokens.refresh_token, "r1");
    assert_eq!(screen::role(&tokens.id_token), "user");
    let error = |error: &str, description: &str| serde_json::json!({"error": error, "error_description": description}).to_string();
    assert_eq!(screen::classify(400, &error("invalid_grant", "The username or the password is wrong.")), Err(Outcome::Wrong));
    assert_eq!(screen::classify(400, &error("invalid_request", "t-alice already exists.")), Err(Outcome::Taken));
    assert_eq!(screen::classify(400, r#"{"error":"password_change_required"}"#), Err(Outcome::MustChange));
    assert_eq!(screen::classify(503, &error("accounts_unavailable", "down")), Err(Outcome::Unreachable));
    assert_eq!(screen::classify(502, "<html>"), Err(Outcome::Unreachable));
    assert_eq!(
        screen::classify(400, &error("invalid_request", "Passwords are at most 1024 characters.")),
        Err(Outcome::Failed("Passwords are at most 1024 characters.".to_string()))
    );
    assert_eq!(screen::classify(500, ""), Err(Outcome::Failed("The accounts server answered 500.".to_string())));
    assert!(matches!(screen::classify(200, "{}"), Err(Outcome::Failed(_))));
}

#[test]
fn the_grant_body_is_the_password_grant() {
    let grant = Grant { username: "t-alice".to_string(), password: String::new(), create: true, new_password: Some(" n ".to_string()) };
    let body = screen::grant_body(&grant, "laptop");
    assert_eq!(body["grant_type"], "password");
    assert_eq!(body["client_id"], screen::CLIENT_ID);
    assert_eq!(body["password"], "");
    assert_eq!(body["create"], true);
    assert_eq!(body["new_password"], " n ");
    assert_eq!(body["machine"], "laptop");
    let plain = screen::grant_body(&Grant::default(), "laptop");
    assert!(plain.get("create").is_none() && plain.get("new_password").is_none());
}

#[test]
fn a_wrong_password_clears_only_the_password() {
    let mut screen = Screen::new(&node("prime", false));
    press(&mut screen, KeyCode::Char('1'));
    typed(&mut screen, "t-alice");
    press(&mut screen, KeyCode::Enter);
    typed(&mut screen, "nope");
    let Step::Send(grant) = press(&mut screen, KeyCode::Enter) else { panic!("not sent") };
    screen.apply(&grant, Outcome::Wrong);
    let View::Form(form) = &screen.view else { panic!("not back at the form") };
    assert_eq!(form.fields, vec!["t-alice".to_string(), String::new()]);
    assert_eq!(form.focus, 1);
    assert!(text(&screen).ends_with("Wrong username or password."));
}

#[test]
fn a_taken_name_returns_to_the_create_form() {
    let mut screen = Screen::new(&node("prime", false));
    press(&mut screen, KeyCode::Char('2'));
    typed(&mut screen, "t-alice");
    press(&mut screen, KeyCode::Enter);
    typed(&mut screen, "pw");
    press(&mut screen, KeyCode::Enter);
    typed(&mut screen, "pw");
    let Step::Send(grant) = press(&mut screen, KeyCode::Enter) else { panic!("not sent") };
    assert!(grant.create);
    screen.apply(&grant, Outcome::Taken);
    assert_eq!(screen.error.as_deref(), Some("That username is taken."));
    assert!(matches!(&screen.view, View::Form(form) if form.kind == Kind::Create && form.focus == 0));
}

#[test]
fn a_must_change_sign_in_asks_for_the_new_password_and_resends_the_old_one() {
    let mut screen = Screen::new(&node("prime", false));
    press(&mut screen, KeyCode::Char('1'));
    typed(&mut screen, "t-alice");
    press(&mut screen, KeyCode::Enter);
    typed(&mut screen, "one-time");
    let Step::Send(grant) = press(&mut screen, KeyCode::Enter) else { panic!("not sent") };
    screen.apply(&grant, Outcome::MustChange);
    assert!(text(&screen).starts_with("  Set a new password for t-alice"));
    assert!(screen.typing());
    typed(&mut screen, "new");
    press(&mut screen, KeyCode::Enter);
    typed(&mut screen, "new!");
    assert_eq!(press(&mut screen, KeyCode::Enter), Step::Redraw);
    assert_eq!(screen.error.as_deref(), Some("The passwords do not match."));
    press(&mut screen, KeyCode::Down);
    press(&mut screen, KeyCode::Backspace);
    let step = press(&mut screen, KeyCode::Enter);
    let expected = Grant {
        username: "t-alice".to_string(),
        password: "one-time".to_string(),
        create: false,
        new_password: Some("new".to_string()),
    };
    assert_eq!(step, Step::Send(expected));
}

#[test]
fn an_unreachable_primary_offers_r_to_retry() {
    let mut screen = Screen::new(&node("prime", false));
    press(&mut screen, KeyCode::Char('1'));
    typed(&mut screen, "t-alice");
    let Step::Send(grant) = ({
        press(&mut screen, KeyCode::Enter);
        press(&mut screen, KeyCode::Enter)
    }) else {
        panic!("not sent")
    };
    screen.apply(&grant, Outcome::Unreachable);
    assert!(matches!(screen.view, View::Offline(_)));
    assert!(!screen.typing(), "r is a key here, not text");
    assert!(text(&screen).contains("box cannot reach its accounts server prime. Try again or ask the node's admin."));
    assert_eq!(press(&mut screen, KeyCode::Char('r')), Step::Send(grant));
}

#[test]
fn the_first_admin_line_shows_after_a_create_and_enter_completes_the_step() {
    let grant = Grant { username: "t-alice".to_string(), create: true, ..Grant::default() };
    for (upgrade, phones) in [(false, false), (true, true)] {
        let mut screen = Screen::new(&node("prime", upgrade));
        if upgrade {
            assert_eq!(screen.view, View::Note);
            assert!(text(&screen).contains("Ragon now has accounts. Sign in or create one on box;"));
            assert_eq!(press(&mut screen, KeyCode::Enter), Step::Redraw);
        }
        screen.view = View::Working(screen::Form { kind: Kind::Create, fields: vec![String::new(); 3], focus: 0 });
        screen.apply(&grant, Outcome::Signed { admin: true });
        let shown = text(&screen);
        assert!(shown.contains("✓ Signed in to prime as t-alice"));
        assert!(shown.contains("You are the first user of prime and its admin. Others can create accounts at the login screen; /users manages them."));
        assert_eq!(shown.contains("Phones need to pair again."), phones);
        assert_eq!(press(&mut screen, KeyCode::Enter), Step::Done);
        assert_eq!(text(&screen), "✓ Signed in to prime as t-alice");
    }
    let mut screen = Screen::new(&node("prime", false));
    screen.view = View::Working(screen::Form { kind: Kind::SignIn, fields: vec![String::new(); 2], focus: 0 });
    screen.apply(&Grant { username: "t-alice".to_string(), ..Grant::default() }, Outcome::Signed { admin: false });
    assert!(!text(&screen).contains("first user"));
}

#[test]
fn a_password_goes_only_to_https_or_this_machine() {
    assert!(screen::may_send("https://box.local:8000"));
    assert!(screen::may_send("http://127.0.0.1:18099"));
    assert!(screen::may_send("http://localhost:8000/"));
    assert!(!screen::may_send("http://box.local:8000"));
    assert!(!screen::may_send("http://192.0.2.4:8000"));
    let outcome = screen::sign_in(&Node { gate: "http://box.local:8000".to_string(), ..node("prime", false) }, &Grant::default());
    assert!(matches!(outcome, Outcome::Failed(text) if text.contains("plain HTTP")));
}

#[test]
fn the_tokens_are_saved_where_codex_reads_them() {
    let home = std::env::temp_dir().join(format!("ragon-auth-screen-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    let tokens = screen::classify(200, &tokens_for("admin")).unwrap();
    assert_eq!(screen::role(&tokens.id_token), "admin");
    screen::save(&home, &tokens).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(home.join("auth.json")).unwrap()).unwrap();
    let entry = saved.get("entries").and_then(|entries| entries.as_object()).and_then(|e| e.values().next()).unwrap_or(&saved);
    assert_eq!(entry["auth_mode"], "chatgpt");
    assert_eq!(entry["tokens"]["refresh_token"], "r1");
    assert_eq!(entry["tokens"]["account_id"], "u1");
    assert_eq!(entry["tokens"]["id_token"], tokens.id_token);
    std::fs::remove_dir_all(&home).unwrap();
}

#[test]
fn a_home_used_before_accounts_gets_the_note() {
    let home = std::env::temp_dir().join(format!("ragon-auth-screen-home-{}", std::process::id()));
    std::fs::create_dir_all(home.join("sessions")).unwrap();
    assert!(!screen::used_before(&home), "an empty sessions folder is a fresh home");
    std::fs::write(home.join("sessions").join("x.jsonl"), "").unwrap();
    assert!(screen::used_before(&home));
    std::fs::remove_dir_all(&home).unwrap();
}
