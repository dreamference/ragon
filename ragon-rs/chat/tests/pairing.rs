//! Pairing a chat identity to an account through a stand-in primary (an axum server on
//! loopback): the redeem call, the binding file, the upgrade drop and the daily hint.
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::routing::post;
use ragon_chat::bindings::Bindings;
use ragon_chat::bindings::Refused;
use ragon_chat::bindings::accounts_mode;
use ragon_chat::bindings::{REPAIRED_MARKER, drop_old_pairings};
use ragon_chat::bindings::hint_due;
use ragon_chat::bindings::instruction;
use ragon_chat::bindings::pair;
use ragon_chat::bindings::settle_pairings;
use ragon_chat::bindings::write_primary;
use serde_json::Value;
use serde_json::json;

type Seen = Arc<Mutex<Vec<(String, Value)>>>;

async fn redeem(State(seen): State<Seen>, headers: HeaderMap, Json(body): Json<Value>) -> (StatusCode, Json<Value>) {
    let auth = headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
    seen.lock().unwrap().push((auth, body.clone()));
    if body["code"] == "ABC234" {
        (StatusCode::OK, Json(json!({ "id": "b1", "username": "ada", "key": "sk-ragon-fresh" })))
    } else {
        (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid_code" })))
    }
}

async fn setup(tag: &str) -> (PathBuf, PathBuf, Seen) {
    let seen = Seen::default();
    let app = Router::new().route("/ragon/v1/bindings/redeem", post(redeem)).with_state(seen.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let gate = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let home = std::env::temp_dir().join(format!("ragon-pairing-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    let dir = home.join(".ragon").join("chat");
    std::fs::create_dir_all(home.join(".config/dreamference")).unwrap();
    std::fs::write(home.join(".config/dreamference/service-key"), "sk-ragon-service\n").unwrap();
    write_primary(&dir, &gate, "node-one", None).unwrap();
    (home, dir, seen)
}

#[tokio::test]
async fn a_good_code_binds_the_chat_to_the_account_and_keeps_the_key_private() {
    let (home, dir, seen) = setup("good").await;
    let (redeemed, line) = pair(&dir, &home, "telegram", "ABC234", "42", "telegram:42").await.unwrap();
    assert_eq!((redeemed.username.as_str(), redeemed.key.as_str()), ("ada", "sk-ragon-fresh"));
    assert_eq!(line, "Paired to ada on node-one");
    let calls = seen.lock().unwrap().clone();
    assert_eq!(calls[0].0, "Bearer sk-ragon-service");
    assert_eq!(calls[0].1, json!({ "code": "ABC234", "chat_kind": "telegram", "chat_identity": "42" }));
    let bindings = Bindings::load(&dir);
    assert_eq!(bindings.for_chat("telegram:42").unwrap().key, "sk-ragon-fresh");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.join("bindings.json")).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}

#[tokio::test]
async fn a_wrong_code_binds_nothing_and_an_unconfigured_bridge_cannot_pair() {
    let (home, dir, _seen) = setup("bad").await;
    assert_eq!(pair(&dir, &home, "telegram", "ZZZ999", "42", "telegram:42").await.unwrap_err(), Refused::Invalid);
    assert!(Bindings::load(&dir).is_empty());
    let empty = home.join("elsewhere").join(".ragon").join("chat");
    assert_eq!(pair(&empty, &home, "telegram", "ABC234", "42", "telegram:42").await.unwrap_err(), Refused::Unreachable);
}

#[tokio::test]
async fn the_upgrade_drops_every_old_pairing_once() {
    let (_home, dir, _seen) = setup("drop").await;
    std::fs::write(dir.join("telegram.json"), json!({ "token": "t", "users": [1, 2] }).to_string()).unwrap();
    std::fs::write(dir.join("discord.json"), json!({ "token": "d", "owner": "9", "users": { "3": "c" } }).to_string()).unwrap();
    assert!(drop_old_pairings(&dir));
    let telegram: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("telegram.json")).unwrap()).unwrap();
    assert_eq!(telegram["token"], "t");
    assert!(telegram["users"].as_array().is_none_or(Vec::is_empty));
    let discord: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("discord.json")).unwrap()).unwrap();
    assert!(discord["users"].as_object().is_none_or(serde_json::Map::is_empty));
    assert!(!drop_old_pairings(&dir), "the marker makes the drop a one-time step");
}

#[tokio::test]
async fn the_drop_backs_up_what_it_removes_and_clears_the_repair_marker() {
    let (_home, dir, _seen) = setup("backup").await;
    let telegram = json!({ "token": "t", "users": [1, 2] }).to_string();
    let discord = json!({ "token": "d", "owner": "9", "users": { "3": "c" } }).to_string();
    let bindings = json!({ "bindings": { "telegram:5": { "username": "u", "key": "k", "chat": "5" } } }).to_string();
    std::fs::write(dir.join("telegram.json"), &telegram).unwrap();
    std::fs::write(dir.join("discord.json"), &discord).unwrap();
    std::fs::write(dir.join("bindings.json"), &bindings).unwrap();
    std::fs::write(dir.join(REPAIRED_MARKER), "0.9.9").unwrap();
    assert!(drop_old_pairings(&dir));
    let backups = |prefix: &str| -> Vec<std::path::PathBuf> {
        std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with(prefix))
            .collect()
    };
    for (name, original) in [("telegram.json.bak-", &telegram), ("discord.json.bak-", &discord), ("bindings.json.bak-", &bindings)] {
        let found = backups(name);
        assert_eq!(found.len(), 1, "one dated backup of {name}");
        let date = found[0].file_name().unwrap().to_string_lossy().trim_start_matches(name).to_string();
        assert_eq!(date.len(), 10, "a YYYY-MM-DD date: {date}");
        assert_eq!(&std::fs::read_to_string(&found[0]).unwrap(), original);
    }
    assert!(!dir.join("bindings.json").exists());
    assert!(!dir.join(REPAIRED_MARKER).exists(), "the informational marker goes after the real drop");
    // A second run changes nothing: no new backup, the emptied lists are not backed up over.
    let before = backups("").len();
    assert!(!drop_old_pairings(&dir));
    assert_eq!(backups("").len(), before);
    let kept = backups("telegram.json.bak-");
    assert_eq!(std::fs::read_to_string(&kept[0]).unwrap(), telegram);
}

#[tokio::test]
async fn an_unpaired_chat_is_told_how_once_a_day() {
    let (_home, dir, _seen) = setup("hint").await;
    assert!(hint_due(&dir, "telegram:7", 1_000));
    assert!(!hint_due(&dir, "telegram:7", 2_000));
    assert!(hint_due(&dir, "telegram:8", 2_000));
    assert!(hint_due(&dir, "telegram:7", 1_000 + 86_400 + 1));
    assert!(instruction("telegram").contains("ragon pair telegram"));
}

fn legacy_folder(dir: &std::path::Path) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("telegram.json"), json!({ "token": "t", "users": [1, 2] }).to_string()).unwrap();
    std::fs::write(dir.join("discord.json"), json!({ "token": "d", "owner": "9", "users": { "3": "c" } }).to_string()).unwrap();
}

fn telegram_users(dir: &std::path::Path) -> Value {
    serde_json::from_str::<Value>(&std::fs::read_to_string(dir.join("telegram.json")).unwrap()).unwrap()["users"].clone()
}

/// Without accounts nothing is dropped and the one-time marker is not spent; without an answer
/// from the gate nothing on disk changes at all (AUTH §14.1).
#[tokio::test]
async fn without_accounts_or_without_an_answer_the_pairings_and_the_marker_stay() {
    let (_home, dir, _seen) = setup("keep").await;
    legacy_folder(&dir);
    assert_eq!(settle_pairings(&dir, Some(false)), None);
    assert_eq!(settle_pairings(&dir, None), None);
    assert_eq!(telegram_users(&dir), json!([1, 2]));
    assert!(!dir.join("bindings.v2").exists(), "the marker is not consumed while accounts are off");
    assert!(!accounts_mode(&dir, None));
    assert!(!accounts_mode(&dir, Some(false)));
    assert!(accounts_mode(&dir, Some(true)));
    // Accounts arrive: the drop runs then, once.
    assert!(settle_pairings(&dir, Some(true)).is_some());
    assert_eq!(telegram_users(&dir), json!([]));
    assert!(settle_pairings(&dir, Some(true)).is_none());
    // A later start without an answer stays in account mode.
    assert!(accounts_mode(&dir, None));
}

/// A node that ran 0.9.8 without accounts: its marker is moved aside (kept, not deleted), so the
/// real drop runs when accounts arrive; what 0.9.8 emptied stays as it is, and a second start does
/// nothing.
#[tokio::test]
async fn the_repair_moves_the_098_marker_aside_once_and_the_real_drop_runs_later() {
    let (_home, dir, _seen) = setup("repair").await;
    legacy_folder(&dir);
    std::fs::write(dir.join("bindings.v2"), "0.9.9").unwrap();
    // No answer from the gate: nothing moves.
    assert_eq!(settle_pairings(&dir, None), None);
    assert!(dir.join("bindings.v2").exists());
    assert!(!accounts_mode(&dir, None));
    let line = settle_pairings(&dir, Some(false)).expect("the repair says what happened");
    assert!(line.contains("ragon chat telegram pair") && line.contains("cannot be restored"), "{line}");
    assert!(!dir.join("bindings.v2").exists());
    assert_eq!(std::fs::read_to_string(dir.join("bindings.v2.dropped-0.9.8")).unwrap(), "0.9.9");
    assert_eq!(telegram_users(&dir), json!([1, 2]), "the repair touches no list");
    assert_eq!(settle_pairings(&dir, Some(false)), None, "the repair runs once");
    assert!(settle_pairings(&dir, Some(true)).is_some(), "the real drop runs when accounts arrive");
    assert_eq!(std::fs::read_to_string(dir.join("bindings.v2")).unwrap().trim(), "accounts");
}

/// A 0.9.8 marker on a node with bound chats is left alone: that node paired through accounts.
#[tokio::test]
async fn the_repair_leaves_a_node_with_bound_chats_alone() {
    let (home, dir, _seen) = setup("bound").await;
    pair(&dir, &home, "telegram", "ABC234", "42", "telegram:42").await.unwrap();
    std::fs::write(dir.join("bindings.v2"), "0.9.9").unwrap();
    assert_eq!(settle_pairings(&dir, Some(false)), None);
    assert!(dir.join("bindings.v2").exists());
    assert!(accounts_mode(&dir, None));
    assert_eq!(Bindings::load(&dir).len(), 1);
}

/// A node that ran 0.9.8 with accounts on keeps the chats bound since: its 0.9.9 marker counts as
/// the drop already done.
#[tokio::test]
async fn with_accounts_after_098_the_bindings_made_since_stay() {
    let (home, dir, _seen) = setup("after").await;
    pair(&dir, &home, "telegram", "ABC234", "42", "telegram:42").await.unwrap();
    std::fs::write(dir.join("bindings.v2"), "0.9.9").unwrap();
    assert_eq!(settle_pairings(&dir, Some(true)), None);
    assert_eq!(std::fs::read_to_string(dir.join("bindings.v2")).unwrap(), "0.9.9");
    assert_eq!(Bindings::load(&dir).len(), 1);
}
