//! Metal launch recipes per Mac memory size (engine Metal spec §4.2), read from
//! `vectors/metal-recipes.json`, the export of the Python `MetalRecipes`.
//!
//! `pick` returns the recipe of the largest listed size that fits the machine's RAM; 128 GB is the
//! Mac node's gate recipe. `weights_sha256` is the slot for the converted weights' digest (spec §5).

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct MetalRecipe {
    pub memory_gb: u32,
    pub gpu_cap_gb: u32,
    pub sessions: u32,
    #[serde(default)]
    pub batch: Option<u32>,
    pub max_context: u32,
    pub prefill_chunk: u32,
    pub vision: bool,
    pub sparse_head_dir: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MetalRecipes {
    pub model: String,
    pub weights_sha256: Option<String>,
    pub gate_memory_gb: u32,
    pub recipes: Vec<MetalRecipe>,
}

impl MetalRecipe {
    /// The Metal ragon-serve's arguments for this recipe after the program name: the model, the
    /// drafter with its sparse head (`sparse_head_dir` under `head_root`, the snapshot the recipe's
    /// directory is relative to), the context, the prefill chunk and the session slots. `--sessions`
    /// is what lets requests run at once: each session is a slot of its own and the serve interleaves
    /// their decode steps, so a second fork starts while the first still decodes. `vision` and `batch`
    /// have no Metal flag yet and are left out.
    pub fn serve_args(
        &self,
        model_dir: &str,
        draft_dir: &str,
        head_root: &str,
        host: &str,
        port: u16,
    ) -> Vec<String> {
        let head = std::path::Path::new(head_root).join(&self.sparse_head_dir);
        [
            ("--model", model_dir.to_string()),
            ("--host", host.to_string()),
            ("--port", port.to_string()),
            ("--max-context", self.max_context.to_string()),
            ("--chunk", self.prefill_chunk.to_string()),
            ("--sessions", self.sessions.to_string()),
            ("--draft", draft_dir.to_string()),
            ("--sparse-head-dir", head.to_string_lossy().into_owned()),
        ]
        .into_iter()
        .flat_map(|(flag, value)| [flag.to_string(), value])
        .collect()
    }
}

impl MetalRecipes {
    /// Parses the export.
    pub fn parse(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    /// The recipe for `memory_gb` of RAM: the largest listed size not above it, None below the smallest.
    pub fn pick(&self, memory_gb: u32) -> Option<&MetalRecipe> {
        self.recipes.iter().filter(|r| r.memory_gb <= memory_gb).max_by_key(|r| r.memory_gb)
    }

    /// The gate recipe (MN5).
    pub fn gate(&self) -> Option<&MetalRecipe> {
        self.recipes.iter().find(|r| r.memory_gb == self.gate_memory_gb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> MetalRecipes {
        MetalRecipes::parse(include_str!("../vectors/metal-recipes.json")).unwrap()
    }

    #[test]
    fn picks_the_64_gb_recipe() {
        let r = fixture();
        let p = r.pick(64).unwrap();
        assert_eq!((p.sessions, p.max_context, p.vision), (2, 131072, true));
        assert_eq!(r.pick(96).unwrap().memory_gb, 64);
    }

    #[test]
    fn picks_the_128_gb_gate_recipe() {
        let r = fixture();
        let p = r.pick(128).unwrap();
        assert_eq!((p.sessions, p.batch, p.max_context), (4, Some(4), 262144));
        assert_eq!(Some(p), r.gate());
        assert_eq!(p.sparse_head_dir, "sparse-head");
        assert!(r.weights_sha256.is_none());
    }

    #[test]
    fn serve_args_carry_the_sessions() {
        let r = fixture();
        let args = r
            .gate()
            .unwrap()
            .serve_args("/m", "/d", "/d", "127.0.0.1", 18080);
        let value = |flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .map(|i| args[i + 1].as_str())
        };
        assert_eq!(value("--sessions"), Some("4"));
        assert_eq!(value("--max-context"), Some("262144"));
        assert_eq!(value("--chunk"), Some("2048"));
        assert_eq!(value("--sparse-head-dir"), Some("/d/sparse-head"));
        assert_eq!(
            r.pick(64)
                .unwrap()
                .serve_args("/m", "/d", "/d", "127.0.0.1", 1)[11],
            "2"
        );
    }

    #[test]
    fn nothing_below_the_smallest_size() {
        assert!(fixture().pick(16).is_none());
    }
}
