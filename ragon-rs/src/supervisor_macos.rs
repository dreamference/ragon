//! `ragon server start|stop|status|run|install|uninstall` on macOS: the Mac node's model server
//! under a supervisor (owner ruling MN1 (b); specs/DREAMFERENCE_MAC_NODE.md §4.2). The supervisor is
//! the `ragon-supervisor` crate (ragon-rs/supervisor), the same platform-neutral core
//! (supervisor/src/core.rs) as the Windows node; its macOS parts are the LaunchDaemon
//! `ai.dreamference.ragon-node` run as `_ragon` (supervisor/src/macos_launchd.rs), the power
//! settings on mains (macos_power.rs) and the guard that takes the engine down with a dead
//! supervisor (macos_guard.rs). This module hands the arguments after `server` to it, with the
//! node's hooks for `server run`: the Bonjour registration (src/node_advertise_macos.rs) follows the
//! supervisor's state, withdrawn while admission is closed (on battery below the threshold, MN6),
//! and `ragon web serve --lan` runs as the supervisor's sidecar when the node advertises its web UI.
//!
//! Elsewhere it is not used: Windows goes through src/supervisor_windows.rs, Linux has
//! `ragon-admin server start`.

/// Runs `ragon server <args>` and returns the process exit code.
pub fn run_cli(args: &[String]) -> i32 {
    #[cfg(target_os = "macos")]
    {
        // `run`, `install` and `start` with no engine after `--` get the node's Metal serve
        // (src/mac_engine.rs); an installed service's `start` runs as installed.
        let installed = |name: &str| {
            ragon_supervisor::macos_launchd::installed_kind(name, |p| p.exists(), None).is_some()
        };
        let args = match crate::mac_engine::with_engine(args, crate::mac_engine::memory_gb, installed, |p| p.exists())
        {
            Ok(args) => args,
            Err(why) => {
                eprintln!("ragon server: {why}");
                return 2;
            }
        };
        match ragon_supervisor::cli::parse(&args) {
            Ok(opts) if opts.command == "run" => {
                ragon_supervisor::cli::run_with(&args, &mut mac::MacNode::new(opts.port))
            }
            _ => ragon_supervisor::cli::run(&args),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = args;
        eprintln!("`ragon server` here is the Mac node's supervisor; it runs on macOS only.");
        2
    }
}

/// The web UI's port on a node (`ragon web`'s default).
pub const WEB_PORT: u16 = 3100;

/// The advertised `state` for the supervisor's reported one: a serving engine is `ready` (on
/// battery too: admission decides whether it is advertised at all), one on its way `loading`,
/// anything else (stopped, refused) `stopped`.
pub fn advertised_state(reported: &str) -> &'static str {
    match reported {
        "healthy" | "battery" => "ready",
        "starting" | "unhealthy" | "waiting" => "loading",
        _ => "stopped",
    }
}

/// What the node advertises for a reported state, or None when it advertises nothing: no node id
/// (not a node), or admission closed (MN6: no new sessions below the charge threshold).
pub fn advertisement(
    node_id: Option<&str>,
    version: &str,
    port: u16,
    web: bool,
    remote: Option<String>,
    reported: &str,
    admitting: bool,
) -> Option<crate::node_advertise_macos::Advertisement> {
    let node_id = node_id.map(str::trim).filter(|id| !id.is_empty())?;
    if !admitting {
        return None;
    }
    Some(crate::node_advertise_macos::Advertisement {
        node_id: node_id.to_string(),
        version: version.to_string(),
        port,
        web_port: web.then_some(WEB_PORT),
        state: advertised_state(reported).to_string(),
        remote,
        ..Default::default()
    })
}

/// The supervisor's sidecars: `ragon web serve --lan` when the node advertises its web UI.
pub fn sidecars(ragon: &std::path::Path, web: bool) -> Vec<(String, Vec<String>)> {
    if !web {
        return Vec::new();
    }
    let argv = [ragon.display().to_string(), "web".into(), "serve".into(), "--lan".into()];
    vec![("web".to_string(), argv.to_vec())]
}

#[cfg(target_os = "macos")]
mod mac {
    use crate::node_advertise_macos::{Advertisement, Registration};
    use std::path::PathBuf;

    /// The Mac node's hooks for `server run`.
    pub struct MacNode {
        home: PathBuf,
        port: u16,
        web: bool,
        advertised: Option<Advertisement>,
        registration: Option<Registration>,
    }

    impl MacNode {
        pub fn new(port: u16) -> Self {
            let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
            let web = ragon_web_server::cli::advertised_web(&home);
            MacNode { home, port, web, advertised: None, registration: None }
        }
    }

    impl ragon_supervisor::cli::NodeHooks for MacNode {
        fn canary(&self) -> bool {
            true
        }

        fn sidecars(&self) -> Vec<(String, Vec<String>)> {
            match std::env::current_exe() {
                Ok(ragon) => super::sidecars(&ragon, self.web),
                Err(e) => {
                    eprintln!("server run: web: {e}");
                    Vec::new()
                }
            }
        }

        fn on_state(&mut self, state: &str, admitting: bool) {
            let node_id = std::fs::read_to_string(self.home.join(".config/dreamference/node-id")).ok();
            let ad = super::advertisement(
                node_id.as_deref(),
                crate::update::RAGON_VERSION.unwrap_or(""),
                self.port,
                self.web,
                ragon_web_server::cli::overlay_name(&self.home),
                state,
                admitting,
            );
            if ad == self.advertised {
                return;
            }
            if let Some(old) = self.registration.take() {
                old.stop();
            }
            self.registration = match ad.as_ref().map(Registration::start) {
                Some(Ok(reg)) => reg,
                Some(Err(e)) => {
                    eprintln!("server run: bonjour: {e}");
                    None
                }
                None => None,
            };
            self.advertised = ad;
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_bad_call_exits_2_everywhere() {
        // macOS: the usage (`--help` is not a command); elsewhere: the "macOS only" notice.
        assert_eq!(super::run_cli(&["--help".to_string()]), 2);
    }

    #[test]
    fn the_advertised_state_follows_the_supervisor() {
        for (reported, advertised) in [
            ("healthy", "ready"),
            ("battery", "ready"),
            ("starting", "loading"),
            ("unhealthy", "loading"),
            ("waiting", "loading"),
            ("refused", "stopped"),
            ("stopped", "stopped"),
        ] {
            assert_eq!(super::advertised_state(reported), advertised, "{reported}");
        }
    }

    #[test]
    fn nothing_is_advertised_without_a_node_id_or_with_admission_closed() {
        let ad =
            |id: Option<&str>, admitting| super::advertisement(id, "0.9.9", 8000, true, None, "healthy", admitting);
        let open = ad(Some("node-a\n"), true).unwrap();
        assert_eq!((open.node_id.as_str(), open.port, open.web_port), ("node-a", 8000, Some(3100)));
        assert_eq!((open.state.as_str(), open.main, open.forge), ("ready", false, None));
        assert_eq!(ad(Some("node-a"), false), None, "admission closed: the node is withdrawn");
        assert_eq!(ad(None, true), None);
        assert_eq!(ad(Some(" \n"), true), None);
        let no_web = super::advertisement(Some("n"), "", 8000, false, Some("box".into()), "starting", true).unwrap();
        assert_eq!((no_web.web_port, no_web.state.as_str(), no_web.remote.as_deref()), (None, "loading", Some("box")));
    }

    #[test]
    fn the_web_ui_is_a_sidecar_only_when_advertised() {
        let ragon = std::path::Path::new("/Applications/Ragon/bin/ragon");
        assert_eq!(super::sidecars(ragon, false), Vec::new());
        assert_eq!(
            super::sidecars(ragon, true),
            [(
                "web".to_string(),
                vec!["/Applications/Ragon/bin/ragon".to_string(), "web".into(), "serve".into(), "--lan".into()]
            )]
        );
    }
}
