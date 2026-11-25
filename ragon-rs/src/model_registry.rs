//! The default model's launch recipe and release-asset hashes, read from
//! `vectors/model-registry.json`, the export of the Python model registry
//! (`dreamference/hardware/registry_export.py`). A node with no Python reads this.
//!
//! Field names are the registry's own. Only the fields a node needs are typed; the rest of a
//! recipe stays in the file and is ignored here. A file whose `schema_version` is newer than
//! `SCHEMA_VERSION` is refused rather than half read.

use std::collections::BTreeMap;

use serde::Deserialize;

/// The newest export format this reader understands.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct SpeculativeConfig {
    pub model: String,
    pub revision: String,
    pub num_speculative_tokens: u32,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct EngineAsset {
    pub repo: String,
    pub revision: String,
    pub engine: String,
    pub feature: String,
    pub flag: String,
}

/// One engine's launch recipe (`engines.<name>` in the export).
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Recipe {
    pub engine: String,
    #[serde(default)]
    pub docker_image: Option<String>,
    #[serde(default)]
    pub engine_commit: Option<String>,
    #[serde(default)]
    pub engine_features: Vec<String>,
    #[serde(default)]
    pub engine_assets: Vec<EngineAsset>,
    pub revision: String,
    pub max_model_len: u32,
    #[serde(default)]
    pub sessions: Option<u32>,
    #[serde(default)]
    pub batch: Option<u32>,
    #[serde(default)]
    pub kv_pages: Option<bool>,
    #[serde(default)]
    pub vision: Option<bool>,
    #[serde(default)]
    pub extra_args: Vec<String>,
    #[serde(default)]
    pub speculative_config: Option<SpeculativeConfig>,
}

/// One Metal (Mac) recipe (`metal.<memory class>` in the export).
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct MetalRecipe {
    pub tier: String,
    pub memory_class: u32,
    pub sessions: u32,
    pub max_model_len: u32,
    pub prefill_chunk: u32,
    pub vision: bool,
    pub drafter: String,
    pub prefix_store_gb: u32,
    #[serde(default)]
    pub gpu_cap_gb: Option<u32>,
    #[serde(default)]
    pub os_floor_gb: Option<u32>,
    #[serde(default)]
    pub sparse_head_dir: Option<String>,
    #[serde(default)]
    pub converted_weights_sha256: Option<String>,
}

/// Memory (GB, as sold) from which a Mac is the full tier; below it and from 60 the entry tier.
const METAL_FULL_MIN_GB: u32 = 100;
const METAL_ENTRY_MIN_GB: u32 = 60;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ReleaseFile {
    pub url: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ReleaseSource {
    pub revision: String,
    pub files: BTreeMap<String, ReleaseFile>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Model {
    pub name: String,
    pub hf_repo_id: String,
    pub supports_vision: bool,
    pub min_memory_gb: f64,
    pub max_memory_gb: f64,
    pub pinned_revision: Option<String>,
    pub default_engine: String,
    pub engines: BTreeMap<String, Recipe>,
    pub release_sources: BTreeMap<String, ReleaseSource>,
    /// Metal recipes keyed by memory class ("64", "128"); absent in an older export.
    #[serde(default)]
    pub metal: BTreeMap<String, MetalRecipe>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelRegistry {
    pub schema_version: u32,
    pub models_release_tag: String,
    pub default_model: String,
    pub model: Model,
}

impl ModelRegistry {
    /// Parses the export; refuses a schema newer than this reader's.
    pub fn parse(text: &str) -> Result<Self, String> {
        let reg: Self = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if reg.schema_version > SCHEMA_VERSION {
            return Err(format!(
                "model registry schema {} is newer than the {} this build reads",
                reg.schema_version, SCHEMA_VERSION
            ));
        }
        Ok(reg)
    }

    /// The default model's launch recipe for its default engine.
    pub fn default_recipe(&self) -> Option<&Recipe> {
        self.model.engines.get(&self.model.default_engine)
    }

    /// The Metal recipe for a Mac with `memory_gb` of memory: the 128 class from 100 GB, the 64
    /// class from 60 GB, none below (no node role).
    pub fn metal_recipe(&self, memory_gb: u32) -> Option<&MetalRecipe> {
        let key = if memory_gb >= METAL_FULL_MIN_GB {
            "128"
        } else if memory_gb >= METAL_ENTRY_MIN_GB {
            "64"
        } else {
            return None;
        };
        self.model.metal.get(key)
    }

    /// The SHA-256 of a file of a release-hosted repository (the drafter, the sparse head).
    pub fn release_sha256(&self, repo: &str, file: &str) -> Option<&str> {
        self.model.release_sources.get(repo)?.files.get(file).map(|f| f.sha256.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> ModelRegistry {
        ModelRegistry::parse(include_str!("../vectors/model-registry.json")).unwrap()
    }

    #[test]
    fn reads_the_default_models_recipe() {
        let r = fixture();
        assert_eq!(r.default_model, "qwen3.8-27b-nvfp4-dflash2");
        assert_eq!(r.model.default_engine, "ragon");
        let p = r.default_recipe().unwrap();
        assert_eq!((p.sessions, p.batch, p.max_model_len), (Some(4), Some(4), 196608));
        assert_eq!(p.vision, Some(true));
        assert!(p.engine_features.iter().any(|f| f == "sparse-head"));
        let d = p.speculative_config.as_ref().unwrap();
        assert_eq!(d.num_speculative_tokens, 12);
        assert_eq!(r.model.engines["sglang"].max_model_len, 262144);
    }

    #[test]
    fn reads_the_release_hashes() {
        let r = fixture();
        let sha = r.release_sha256("dreamference/qwen3.8-27b-sparse-head", "model.safetensors");
        assert_eq!(sha.map(str::len), Some(64));
        let drafter = &r.default_recipe().unwrap().speculative_config.as_ref().unwrap().model;
        assert!(r.release_sha256(drafter, "model.safetensors").is_some());
        assert!(r.release_sha256("nobody/nothing", "x").is_none());
    }

    #[test]
    fn picks_the_metal_recipe_by_memory() {
        let r = fixture();
        let entry = r.metal_recipe(64).unwrap();
        assert_eq!((entry.tier.as_str(), entry.sessions, entry.max_model_len), ("entry", 2, 98304));
        assert!(entry.vision && entry.prefix_store_gb == 2 && entry.sparse_head_dir.is_none());
        assert_eq!(entry.prefill_chunk, 2048);
        let full = r.metal_recipe(128).unwrap();
        assert_eq!((full.tier.as_str(), full.sessions, full.max_model_len), ("full", 4, 262144));
        assert!(full.sparse_head_dir.is_some() && full.converted_weights_sha256.is_none());
        assert_eq!(r.metal_recipe(96).unwrap().tier, "entry");
        assert!(r.metal_recipe(32).is_none());
    }

    #[test]
    fn refuses_a_newer_schema() {
        let text = include_str!("../vectors/model-registry.json")
            .replacen("\"schema_version\": 1", "\"schema_version\": 2", 1);
        assert!(ModelRegistry::parse(&text).unwrap_err().contains("newer"));
    }
}
