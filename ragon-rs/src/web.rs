//! `ragon web …`: the Ragon web server (specs/DREAMFERENCE_RAGON_ASK.md §4). The server is
//! the `ragon-web-server` crate (`web/`); this module only tells it where things are: the home
//! folder, this binary (it starts `ragon app-server` and reads `ragon prompt show --composed`), and
//! the model server, named to the app-server it starts so that child never browses for a node.

use std::path::PathBuf;

use crate::node;

/// The model server the app-server child is pointed at: the configured one, or loopback on a node.
/// A client with nothing configured leaves it to the child's own tiers, as `ragon` does by itself.
fn child_env() -> Vec<(String, String)> {
    let inputs = node::inputs();
    let host = inputs.configured.or_else(|| inputs.is_node.then(|| crate::DEFAULT_VLLM_HOST.to_string()));
    host.map(|host| vec![("DREAMFERENCE_VLLM_HOST".to_string(), host)]).unwrap_or_default()
}

/// `ragon web …` from a shell. Returns the exit code.
pub async fn run_cli(args: &[String]) -> i32 {
    let Some(codex_home) = codex_utils_home_dir::find_codex_home().ok().map(|home| home.as_path().to_path_buf()) else {
        eprintln!("Could not resolve CODEX_HOME.");
        return 1;
    };
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        eprintln!("HOME is not set.");
        return 1;
    };
    let ragon = match std::env::current_exe() {
        Ok(path) => path,
        Err(err) => {
            eprintln!("Could not find this program's path: {err}");
            return 1;
        }
    };
    let environment = ragon_web_server::Environment { codex_home, home, ragon, child_env: child_env() };
    ragon_web_server::run_cli(args, environment).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_server_child_is_pointed_at_the_configured_model_server() {
        // One test owns the variable: the environment is shared by the whole test process.
        unsafe { std::env::set_var("DREAMFERENCE_VLLM_HOST", "http://model.example:9000") };
        assert_eq!(child_env(), vec![("DREAMFERENCE_VLLM_HOST".to_string(), "http://model.example:9000".to_string())]);
        unsafe { std::env::remove_var("DREAMFERENCE_VLLM_HOST") };
    }
}
