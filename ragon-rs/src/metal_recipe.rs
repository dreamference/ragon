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
    fn nothing_below_the_smallest_size() {
        assert!(fixture().pick(16).is_none());
    }
}
