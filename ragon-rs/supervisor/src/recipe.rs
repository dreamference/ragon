//! The Windows node's engine recipe: which `ragon-serve` to start and with what arguments when
//! `server start|run|install` is given no engine after `--`.
//!
//! - The engine comes from the release asset `ragon-engine-<version>-windows-gfx1151.zip`
//!   (scripts/package_windows_engine.ps1), unpacked flat into `%ProgramFiles%\Ragon\engine`:
//!   `ragon-serve.exe` with its runtime DLLs beside it, so Windows finds them without any SDK on
//!   PATH.
//! - The weights, the drafter and the sparse drafter head are directories under
//!   `%ProgramData%\Ragon\models` (the node's state layout).
//! - One session, 65536 tokens of context: the shape the HIP lanes measured on the Windows node.
//!
//! Platform-neutral (plain paths and strings), so its tests run everywhere; `--engine-dir` and
//! `--models-dir` point it elsewhere (the stand-in drills, a temp prefix).

use std::path::PathBuf;

/// The engine's file name inside the engine directory.
pub const ENGINE_EXE: &str = if cfg!(windows) { "ragon-serve.exe" } else { "ragon-serve" };
/// The model id the engine serves under (as on the Linux nodes).
pub const SERVED_MODEL_NAME: &str = "RadixArk/Qwen3.8-27B-NVFP4";
/// Directory names under the models directory.
pub const WEIGHTS_DIR: &str = "qwen3.8-27b-nvfp4";
pub const DRAFTER_DIR: &str = "qwen3.8-27b-dflash2-nvfp4-scalesearch";
pub const SPARSE_HEAD_DIR: &str = "qwen3.8-27b-sparse-head";
pub const DEFAULT_SESSIONS: u32 = 1;
pub const DEFAULT_MAX_CONTEXT: u32 = 65536;
pub const DRAFT_BLOCK: u32 = 12;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recipe {
    pub engine_dir: PathBuf,
    pub models_dir: PathBuf,
    pub sessions: u32,
    pub max_context: u32,
}

/// `%ProgramFiles%\Ragon\engine` (Windows); elsewhere there is no default install and the
/// recipe applies only when `--engine-dir` is given.
pub fn default_engine_dir() -> PathBuf {
    let base = std::env::var_os("ProgramFiles").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
    base.join("Ragon").join("engine")
}

/// `%ProgramData%\Ragon\models`, the state layout's models directory.
pub fn default_models_dir() -> PathBuf {
    let base = std::env::var_os("ProgramData").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    base.join("Ragon").join("models")
}

impl Recipe {
    pub fn new(engine_dir: Option<PathBuf>, models_dir: Option<PathBuf>) -> Self {
        Self {
            engine_dir: engine_dir.unwrap_or_else(default_engine_dir),
            models_dir: models_dir.unwrap_or_else(default_models_dir),
            sessions: DEFAULT_SESSIONS,
            max_context: DEFAULT_MAX_CONTEXT,
        }
    }

    pub fn engine(&self) -> PathBuf {
        self.engine_dir.join(ENGINE_EXE)
    }

    fn model(&self, dir: &str) -> PathBuf {
        self.models_dir.join(dir)
    }

    /// The engine and its arguments, for the supervisor's `-- <engine> [args]`.
    pub fn command(&self, host: &str, port: u16) -> Vec<String> {
        let path = |p: PathBuf| p.display().to_string();
        vec![
            path(self.engine()),
            "--model".into(),
            path(self.model(WEIGHTS_DIR)),
            "--served-model-name".into(),
            SERVED_MODEL_NAME.into(),
            "--host".into(),
            host.into(),
            "--port".into(),
            port.to_string(),
            "--max-context".into(),
            self.max_context.to_string(),
            "--sessions".into(),
            self.sessions.to_string(),
            "--draft".into(),
            path(self.model(DRAFTER_DIR)),
            "--draft-block".into(),
            DRAFT_BLOCK.to_string(),
            "--sparse-head-dir".into(),
            path(self.model(SPARSE_HEAD_DIR)),
        ]
    }

    /// The paths the engine needs that are not there, each named, so `server start` refuses
    /// before spawning instead of restarting an engine that cannot start.
    pub fn missing(&self) -> Vec<String> {
        let mut missing = Vec::new();
        let engine = self.engine();
        if !engine.is_file() {
            missing.push(format!("the engine {} (unpack the ragon-engine zip there)", engine.display()));
        }
        for (what, dir) in [("weights", WEIGHTS_DIR), ("drafter", DRAFTER_DIR), ("sparse drafter head", SPARSE_HEAD_DIR)] {
            let path = self.model(dir);
            if !path.is_dir() {
                missing.push(format!("the {what} {}", path.display()));
            }
        }
        missing
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ragon-recipe-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_command_carries_the_windows_recipe() {
        let r = Recipe::new(Some(PathBuf::from("E")), Some(PathBuf::from("M")));
        let words = r.command("127.0.0.1", 8000);
        let joined = |p: &[&str]| p.iter().collect::<PathBuf>().display().to_string();
        assert_eq!(words[0], joined(&["E", ENGINE_EXE]));
        let value = |flag: &str| words.iter().position(|w| w == flag).map(|i| words[i + 1].clone()).unwrap();
        assert_eq!(value("--model"), joined(&["M", WEIGHTS_DIR]));
        assert_eq!(value("--served-model-name"), SERVED_MODEL_NAME);
        assert_eq!(value("--host"), "127.0.0.1");
        assert_eq!(value("--port"), "8000");
        assert_eq!(value("--max-context"), "65536");
        assert_eq!(value("--sessions"), "1");
        assert_eq!(value("--draft"), joined(&["M", DRAFTER_DIR]));
        assert_eq!(value("--draft-block"), "12");
        assert_eq!(value("--sparse-head-dir"), joined(&["M", SPARSE_HEAD_DIR]));
        assert_eq!(words.len(), 19);
    }

    #[test]
    fn missing_paths_are_named_and_a_complete_prefix_has_none() {
        let engine = temp("engine");
        let models = temp("models");
        let r = Recipe::new(Some(engine.clone()), Some(models.clone()));
        let missing = r.missing();
        assert_eq!(missing.len(), 4, "{missing:?}");
        assert!(missing[0].contains(ENGINE_EXE));
        assert!(missing[3].contains(SPARSE_HEAD_DIR));
        std::fs::write(engine.join(ENGINE_EXE), b"").unwrap();
        for dir in [WEIGHTS_DIR, DRAFTER_DIR, SPARSE_HEAD_DIR] {
            std::fs::create_dir_all(models.join(dir)).unwrap();
        }
        assert!(r.missing().is_empty());
        let _ = std::fs::remove_dir_all(engine);
        let _ = std::fs::remove_dir_all(models);
    }

    #[test]
    fn defaults_are_under_program_files_and_program_data() {
        let r = Recipe::new(None, None);
        assert!(r.engine_dir.ends_with(Path::new("Ragon").join("engine")));
        assert!(r.models_dir.ends_with(Path::new("Ragon").join("models")));
        assert_eq!((r.sessions, r.max_context), (1, 65536));
    }
}
