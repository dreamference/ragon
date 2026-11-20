//! The app-server request `account/ragon/signIn` (specs/DREAMFERENCE_AUTH.md §5.8): the password
//! grant or the account creation at the issuer, the entry written into the active primary's slot
//! of `auth.json`, and the answer the desktop app shows. The handler in the app-server (patch
//! 0042) makes one call into [`sign_in`], reloads its `AuthManager` and sends `account/updated`.

use std::path::Path;

use super::commands::connect;

/// JSON-RPC code: the username or password was wrong.
pub const WRONG_CREDENTIALS: i64 = -32001;
/// JSON-RPC code: the name is taken or invalid (`create` only).
pub const NAME_TAKEN: i64 = -32002;
/// JSON-RPC code: the accounts server could not be reached.
pub const UNREACHABLE: i64 = -32003;

/// A refusal, with the code the app-server sends and the screen's own text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// One of the three codes above.
    pub code: i64,
    /// The text the screens show.
    pub message: String,
}

/// Who is signed in after a success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedIn {
    /// The username as the primary recorded it.
    pub username: String,
    /// `admin` or `user`.
    pub role: String,
    /// Always false here: a grant refuses while the must-change flag is set (§5.4).
    pub must_change_password: bool,
}

/// The issuer this process talks to: the variable the launcher sets for Codex, else `RAGON_ISSUER`.
pub fn issuer() -> Option<String> {
    [super::ISSUER_ENV, "RAGON_ISSUER"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .find(|value| !value.is_empty())
}

fn refusal(code: i64, message: impl Into<String>) -> Refusal {
    Refusal { code, message: message.into() }
}

/// Signs in (`create` false) or creates the account (`create` true), username and password as
/// typed, and stores the entry.
///
/// # Errors
/// A [`Refusal`] with the code the app-server answers with.
pub async fn sign_in(codex_home: &Path, create: bool, username: &str, password: &str) -> Result<SignedIn, Refusal> {
    let Some(gate) = issuer() else { return Err(refusal(UNREACHABLE, "No accounts server is set.")) };
    let primary = connect(&gate, codex_home).await.map_err(|error| refusal(UNREACHABLE, format!("{error:#}")))?;
    let done = if create { primary.create_account(username, password).await } else { primary.sign_in(username, password).await };
    match done {
        Err(error) => Err(refusal(UNREACHABLE, format!("{error:#}"))),
        Ok(Err(text)) if text.contains(" answered ") => Err(refusal(UNREACHABLE, text)),
        Ok(Err(text)) if create && (text.starts_with("That username") || text.starts_with("Usernames are")) => Err(refusal(NAME_TAKEN, text)),
        Ok(Err(text)) => Err(refusal(WRONG_CREDENTIALS, text)),
        Ok(Ok(())) => {
            let (username, role) = primary.identity().unwrap_or_else(|| (username.to_string(), "user".to_string()));
            Ok(SignedIn { username, role, must_change_password: false })
        }
    }
}
