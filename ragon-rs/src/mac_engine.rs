//! The Mac node's engine: the Metal `ragon-serve` that `ragon server run|install|start` puts after
//! `--` when no engine is given (specs/DREAMFERENCE_MAC_NODE.md §5: the supervisor runs
//! `ragon-serve` natively). Its arguments are [`MetalRecipe::serve_args`] of the recipe for the
//! machine's memory (src/metal_recipe.rs, the registry's export), with [`NODE_SESSIONS`] slots.
//!
//! Where things are, all under the node's state directory (`--state-dir`, the supervisor's):
//! - the engine: `<state dir>/engine/bin/ragon-serve` when present (a node run as its own user,
//!   no root), else the installed one, `/usr/local/lib/ragon/engine/bin/ragon-serve` (install.sh
//!   unpacks `ragon-engine-macos-arm64-<version>.tar.gz` there, root-owned);
//! - the weights: `<state dir>/model` (a directory or a link to one);
//! - the drafter: `<state dir>/drafter`, its sparse head in the recipe's `sparse_head_dir` under it.
//!
//! Everything here is pure (paths and argument lists) and tested on every platform; reading the
//! machine's memory is the only macOS call.

use crate::metal_recipe::{MetalRecipe, MetalRecipes};
use std::path::{Path, PathBuf};

/// Where install.sh unpacks the engine tarball (root-owned; `engine` links to `engine-<version>`).
pub const INSTALLED_ENGINE_DIR: &str = "/usr/local/lib/ragon/engine";
/// The serve inside an engine directory (the tarball's own layout).
pub const ENGINE_BIN: &str = "bin/ragon-serve";
/// The node's own engine directory under the state directory (checked before the installed one).
pub const STATE_ENGINE_DIR: &str = "engine";
/// The weights under the state directory.
pub const MODEL_DIR: &str = "model";
/// The drafter under the state directory; the recipe's sparse head sits inside it.
pub const DRAFTER_DIR: &str = "drafter";
/// Session slots of the node's serve: two requests run at once (a second fork starts while the
/// first decodes). The 128 GB recipe lists 4; 2 is the node's default until the four-slot runs
/// are measured on the gate box.
pub const NODE_SESSIONS: u32 = 2;

/// The id the serve answers to (`--served-model-name`): the registry entry's Hugging Face repository,
/// as the Linux node's ragon-serve serves it (dreamference/vllm_server/ragon_serve_launch_builder.py),
/// so a client maps it back to `qwen3.8-27b-nvfp4-dflash2`.
pub const SERVED_MODEL: &str = "RadixArk/Qwen3.8-27B-NVFP4";
/// The projection weights (`RAGON_METAL_PROJ`): `q4` converts the NVFP4 checkpoint's FP8 projections
/// to the Mac-native 4-bit format at load, as the measured Metal runs serve; `fp8` keeps them as
/// shipped. The choice between them is the owner's; `RAGON_METAL_PROJ` in the environment of
/// `ragon server run|install|start` overrides this default and is written into the command line.
pub const DEFAULT_PROJ: &str = "q4";
/// The variable the engine reads.
pub const PROJ_ENV: &str = "RAGON_METAL_PROJ";
/// Sets the engine's environment on its command line, so an installed service (launchd starts it
/// with an empty environment) runs it the same; `env` execs the serve in place, so its pid and
/// process name stay the engine's.
const ENV_PROGRAM: &str = "/usr/bin/env";

/// The commands whose engine this module fills in when none is given after `--`.
const NEEDS_ENGINE: [&str; 3] = ["run", "install", "start"];

/// The engine binary for a state directory: the node's own copy first, then the installed one.
pub fn engine_binary(state_dir: &Path, exists: impl Fn(&Path) -> bool) -> Result<PathBuf, String> {
    let candidates = [
        state_dir.join(STATE_ENGINE_DIR).join(ENGINE_BIN),
        Path::new(INSTALLED_ENGINE_DIR).join(ENGINE_BIN),
    ];
    candidates.iter().find(|p| exists(p)).cloned().ok_or_else(|| {
        format!(
            "no Metal engine: neither {} nor {} exists (install.sh --role node puts it in place; or give \
             the engine after `--`)",
            candidates[0].display(),
            candidates[1].display()
        )
    })
}

/// The recipe a node with `memory_gb` of RAM serves: the registry's for that size, with
/// [`NODE_SESSIONS`] slots.
pub fn node_recipe(recipes: &MetalRecipes, memory_gb: u32) -> Result<MetalRecipe, String> {
    let mut recipe = recipes.pick(memory_gb).cloned().ok_or_else(|| {
        let floor = recipes.recipes.iter().map(|r| r.memory_gb).min().unwrap_or(0);
        format!("{memory_gb} GB of memory is below the smallest Metal recipe ({floor} GB)")
    })?;
    recipe.sessions = NODE_SESSIONS;
    Ok(recipe)
}

/// The engine command line (program first) for a node: `env RAGON_METAL_PROJ=<proj>`, the engine,
/// then the recipe's serve arguments with the weights and drafter of `state_dir`, listening where
/// the supervisor probes, under [`SERVED_MODEL`].
pub fn engine_command(
    state_dir: &Path,
    host: &str,
    port: u16,
    memory_gb: u32,
    proj: &str,
    recipes: &MetalRecipes,
    exists: impl Fn(&Path) -> bool,
) -> Result<Vec<String>, String> {
    if !["q4", "fp8"].contains(&proj) {
        return Err(format!("{PROJ_ENV}={proj}: the engine takes q4 or fp8"));
    }
    let program = engine_binary(state_dir, &exists)?;
    let model = state_dir.join(MODEL_DIR);
    let drafter = state_dir.join(DRAFTER_DIR);
    for (what, dir) in [("weights", &model), ("drafter", &drafter)] {
        if !exists(dir) {
            return Err(format!("no {what}: {} does not exist", dir.display()));
        }
    }
    let recipe = node_recipe(recipes, memory_gb)?;
    let head = drafter.join(&recipe.sparse_head_dir);
    if !exists(&head) {
        return Err(format!("no sparse head: {} does not exist", head.display()));
    }
    let (model, drafter) = (model.display().to_string(), drafter.display().to_string());
    let mut argv = vec![ENV_PROGRAM.to_string(), format!("{PROJ_ENV}={proj}"), program.display().to_string()];
    argv.extend(recipe.serve_args(&model, &drafter, &drafter, host, port));
    argv.extend(["--served-model-name".to_string(), SERVED_MODEL.to_string()]);
    Ok(argv)
}

/// `ragon server <args>` with the node's engine after `--` when the command needs one and none is
/// given; any other command, or one that names its engine, comes back unchanged. `installed` says
/// whether the service `name` is installed: `start` then runs it as installed and needs no engine.
/// `proj` is `RAGON_METAL_PROJ` from the caller's environment ([`DEFAULT_PROJ`] when unset).
pub fn with_engine(
    args: &[String],
    memory_gb: impl FnOnce() -> Option<u32>,
    proj: Option<&str>,
    installed: impl FnOnce(&str) -> bool,
    exists: impl Fn(&Path) -> bool,
) -> Result<Vec<String>, String> {
    let Some(command) = args.first() else { return Ok(args.to_vec()) };
    if !NEEDS_ENGINE.contains(&command.as_str()) || args.iter().any(|a| a == "--") {
        return Ok(args.to_vec());
    }
    // The supervisor's own reading of the options (state directory, host, port), from a command
    // that needs no engine.
    let mut probe = args.to_vec();
    probe[0] = "status".to_string();
    let opts = match ragon_supervisor::cli::parse(&probe) {
        Ok(opts) => opts,
        // A bad option: the supervisor says so with its usage.
        Err(_) => return Ok(args.to_vec()),
    };
    if command == "start" && installed(&opts.name) {
        return Ok(args.to_vec());
    }
    let memory_gb = memory_gb().ok_or("could not read the machine's memory (sysctl hw.memsize)")?;
    let proj = proj.unwrap_or(DEFAULT_PROJ);
    let engine =
        engine_command(&opts.state_dir, &opts.host, opts.port, memory_gb, proj, &MetalRecipes::bundled(), exists)?;
    let mut out = args.to_vec();
    out.push("--".to_string());
    out.extend(engine);
    Ok(out)
}

/// The machine's memory in GB (GiB, as the recipes count it), from `sysctl -n hw.memsize`.
pub fn memory_gb() -> Option<u32> {
    let out = std::process::Command::new("/usr/sbin/sysctl").args(["-n", "hw.memsize"]).output().ok()?;
    parse_memsize(&String::from_utf8_lossy(&out.stdout))
}

/// `hw.memsize` bytes -> GiB.
pub fn parse_memsize(text: &str) -> Option<u32> {
    let bytes: u64 = text.trim().parse().ok()?;
    u32::try_from(bytes >> 30).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn s(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    fn present(paths: &[&str]) -> impl Fn(&Path) -> bool {
        let set: HashSet<PathBuf> = paths.iter().map(PathBuf::from).collect();
        move |p: &Path| set.contains(p)
    }

    const NODE: [&str; 4] =
        ["/n/engine/bin/ragon-serve", "/n/model", "/n/drafter", "/n/drafter/sparse-head"];

    fn value<'a>(argv: &'a [String], flag: &str) -> Option<&'a str> {
        argv.iter().position(|a| a == flag).map(|i| argv[i + 1].as_str())
    }

    #[test]
    fn the_node_serves_two_sessions_of_its_memory_recipe() {
        let argv = engine_command(Path::new("/n"), "127.0.0.1", 8000, 128, "q4", &MetalRecipes::bundled(), present(&NODE))
            .unwrap();
        assert_eq!(&argv[..3], ["/usr/bin/env", "RAGON_METAL_PROJ=q4", "/n/engine/bin/ragon-serve"]);
        assert_eq!(value(&argv, "--served-model-name"), Some("RadixArk/Qwen3.8-27B-NVFP4"));
        assert_eq!(value(&argv, "--model"), Some("/n/model"));
        assert_eq!(value(&argv, "--draft"), Some("/n/drafter"));
        assert_eq!(value(&argv, "--sparse-head-dir"), Some("/n/drafter/sparse-head"));
        assert_eq!(value(&argv, "--sessions"), Some("2"));
        assert_eq!(value(&argv, "--host"), Some("127.0.0.1"));
        assert_eq!(value(&argv, "--port"), Some("8000"));
        // The 128 GB recipe's context and chunk; a 64 GB Mac gets its own.
        assert_eq!(value(&argv, "--max-context"), Some("262144"));
        let small = engine_command(Path::new("/n"), "h", 1, 64, "fp8", &MetalRecipes::bundled(), present(&NODE)).unwrap();
        assert_eq!(value(&small, "--max-context"), Some("131072"));
        assert_eq!(value(&small, "--sessions"), Some("2"));
        assert_eq!(small[1], "RAGON_METAL_PROJ=fp8");
    }

    #[test]
    fn the_installed_engine_when_the_node_has_none_of_its_own() {
        let mut paths = NODE[1..].to_vec();
        paths.push("/usr/local/lib/ragon/engine/bin/ragon-serve");
        let argv = engine_command(Path::new("/n"), "h", 1, 128, "q4", &MetalRecipes::bundled(), present(&paths)).unwrap();
        assert_eq!(argv[2], "/usr/local/lib/ragon/engine/bin/ragon-serve");
    }

    #[test]
    fn what_is_missing_is_named() {
        let recipes = MetalRecipes::bundled();
        let err = |paths: &[&str], gb| engine_command(Path::new("/n"), "h", 1, gb, "q4", &recipes, present(paths)).unwrap_err();
        assert!(err(&NODE[1..], 128).contains("no Metal engine"));
        assert!(err(&[NODE[0], NODE[2], NODE[3]], 128).contains("no weights: /n/model"));
        assert!(err(&[NODE[0], NODE[1], NODE[3]], 128).contains("no drafter: /n/drafter"));
        assert!(err(&NODE[..3], 128).contains("no sparse head"));
        assert!(err(&NODE, 16).contains("below the smallest Metal recipe (36 GB)"));
        let bad = engine_command(Path::new("/n"), "h", 1, 128, "q8", &recipes, present(&NODE)).unwrap_err();
        assert!(bad.contains("RAGON_METAL_PROJ=q8"), "{bad}");
    }

    #[test]
    fn run_install_and_start_get_the_engine_after_the_options() {
        for command in ["run", "install", "start"] {
            let args = s(&[command, "--state-dir", "/n", "--port", "18080"]);
            let out = with_engine(&args, || Some(128), None, |_| false, present(&NODE)).unwrap();
            assert_eq!(&out[..5], &args[..]);
            assert_eq!(out[5], "--");
            assert_eq!(out[6], "/usr/bin/env");
            assert_eq!(out[8], "/n/engine/bin/ragon-serve");
            assert_eq!(value(&out, "--port"), Some("18080"));
            // The supervisor reads the whole line back.
            let opts = ragon_supervisor::cli::parse(&out).unwrap();
            assert_eq!(opts.engine[..3], ["/usr/bin/env", "RAGON_METAL_PROJ=q4", "/n/engine/bin/ragon-serve"]);
            assert_eq!(value(&opts.engine, "--sessions"), Some("2"));
        }
    }

    #[test]
    fn a_given_engine_and_other_commands_are_left_alone() {
        let given = s(&["run", "--state-dir", "/n", "--", "/x/standin", "--port", "1"]);
        assert_eq!(with_engine(&given, || None, None, |_| false, present(&[])).unwrap(), given);
        for command in ["stop", "status", "uninstall"] {
            let args = s(&[command, "--state-dir", "/n"]);
            assert_eq!(with_engine(&args, || None, None, |_| false, present(&[])).unwrap(), args);
        }
        assert_eq!(with_engine(&[], || None, None, |_| false, present(&[])).unwrap(), Vec::<String>::new());
        // A bad option is the supervisor's to report, with its usage.
        let bad = s(&["run", "--bogus"]);
        assert_eq!(with_engine(&bad, || None, None, |_| false, present(&[])).unwrap(), bad);
    }

    #[test]
    fn start_of_an_installed_service_needs_no_engine() {
        let args = s(&["start", "--state-dir", "/n"]);
        assert_eq!(with_engine(&args, || None, None, |name| name == "ragon-node", present(&[])).unwrap(), args);
        // Not installed and nothing to run: the reason, not a restart loop.
        let err = with_engine(&args, || Some(128), None, |_| false, present(&[])).unwrap_err();
        assert!(err.contains("no Metal engine"), "{err}");
        let err = with_engine(&s(&["run", "--state-dir", "/n"]), || None, None, |_| false, present(&NODE)).unwrap_err();
        assert!(err.contains("hw.memsize"), "{err}");
    }

    #[test]
    fn memsize_is_counted_in_gib() {
        assert_eq!(parse_memsize("137438953472\n"), Some(128));
        assert_eq!(parse_memsize("68719476736"), Some(64));
        assert_eq!(parse_memsize("nope"), None);
    }
}
