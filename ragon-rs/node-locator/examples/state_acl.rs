//! `state_acl apply|check [PROGRAMDATA_DIR]`: applies or verifies the Windows state directory's ACLs
//! (the table in `ragon_node_locator::state`). The CI step runs it on a temporary folder with
//! `RAGON_STATE_JOB`, `RAGON_STATE_SERVICE` and `RAGON_STATE_SANDBOX` naming accounts that exist.

#[cfg(windows)]
fn main() {
    use ragon_node_locator::state::{Accounts, StateLayout, apply, check};
    let mut args = std::env::args().skip(1);
    let verb = args.next().unwrap_or_default();
    let layout = match args.next() {
        Some(dir) => StateLayout::under(std::path::Path::new(&dir)),
        None => StateLayout::from_env().expect("no ProgramData"),
    };
    let mut accounts = Accounts::default();
    if let Ok(name) = std::env::var("RAGON_STATE_SERVICE") {
        accounts.service = name;
    }
    if let Ok(name) = std::env::var("RAGON_STATE_JOB") {
        accounts.job = name;
    }
    if let Ok(name) = std::env::var("RAGON_STATE_SANDBOX") {
        accounts.sandbox = vec![name];
    }
    let result = match verb.as_str() {
        "apply" => apply(&layout, &accounts).map_err(|error| error.to_string()),
        "check" => check(&layout, &accounts),
        _ => Err("usage: state_acl apply|check [DIR]".to_string()),
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("state_acl runs on Windows only");
    std::process::exit(2);
}
