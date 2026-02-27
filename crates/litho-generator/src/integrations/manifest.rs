//! Documentation Manifest — tracks generated documentation state for incremental builds.
//!
//! After every full generation run, a `DocumentationManifest` is saved to `.litho/manifest.json`.
//! This manifest records which files were processed, what content was generated, and the git state
//! at generation time. The `ChangeDetector` (in `change_detector.rs`) compares the manifest against
//! the current repo state to determine which agents need re-running.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Top-level documentation manifest, persisted as `.litho/manifest.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentationManifest {
    /// Manifest format version (for forward compatibility)
    pub version: u32,
    /// When this manifest was generated
    pub generated_at: DateTime<Utc>,
    /// Git commit hash at generation time (short hash)
    pub git_commit: Option<String>,
    /// Git branch at generation time
    pub git_branch: Option<String>,
    /// Absolute path to the project root
    pub project_path: PathBuf,
    /// BLAKE3 hashes of all source files that were processed
    pub file_hashes: HashMap<PathBuf, String>,
    /// Per-agent/module generation metadata
    pub modules: HashMap<String, ModuleManifest>,
    /// Total wall-clock generation time in seconds
    pub total_generation_time_secs: f64,
}

/// Metadata for a single generated documentation module (one per agent output).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleManifest {
    /// Agent type string (e.g., "SystemContextResearcher", "Overview")
    pub agent_type: String,
    /// Relative path to the generated output file
    pub output_file: String,
    /// Source files that contributed to this module's generation
    pub input_files: Vec<PathBuf>,
    /// When this module was generated
    pub generated_at: DateTime<Utc>,
    /// BLAKE3 hash of the generated output content
    pub content_hash: String,
}

impl DocumentationManifest {
    /// Create a new empty manifest for the given project path.
    pub fn new(project_path: PathBuf) -> Self {
        Self {
            version: 1,
            generated_at: Utc::now(),
            git_commit: None,
            git_branch: None,
            project_path,
            file_hashes: HashMap::new(),
            modules: HashMap::new(),
            total_generation_time_secs: 0.0,
        }
    }

    /// Save the manifest to `.litho/manifest.json` inside the project.
    pub async fn save(&self, internal_path: &Path) -> Result<()> {
        let manifest_path = internal_path.join("manifest.json");
        tokio::fs::create_dir_all(internal_path).await?;
        let json = serde_json::to_string_pretty(self)?;
        tokio::fs::write(&manifest_path, json).await?;
        println!("📋 Manifest saved to {}", manifest_path.display());
        Ok(())
    }

    /// Load a manifest from `.litho/manifest.json`.
    ///
    /// Returns `None` if the file doesn't exist (first run).
    pub async fn load(internal_path: &Path) -> Option<Self> {
        let manifest_path = internal_path.join("manifest.json");
        let content = tokio::fs::read_to_string(&manifest_path).await.ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Record a file hash for a processed source file.
    pub fn record_file_hash(&mut self, path: PathBuf, hash: String) {
        self.file_hashes.insert(path, hash);
    }

    /// Record a generated module's metadata.
    pub fn record_module(
        &mut self,
        agent_key: String,
        agent_type: String,
        output_file: String,
        input_files: Vec<PathBuf>,
        content: &str,
    ) {
        let content_hash = blake3::hash(content.as_bytes()).to_hex().to_string();
        self.modules.insert(
            agent_key,
            ModuleManifest {
                agent_type,
                output_file,
                input_files,
                generated_at: Utc::now(),
                content_hash,
            },
        );
    }
}

/// Compute the BLAKE3 hash of a file's contents.
pub async fn hash_file(path: &Path) -> Result<String> {
    let content = tokio::fs::read(path).await?;
    Ok(blake3::hash(&content).to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_roundtrip() {
        let mut manifest = DocumentationManifest::new(PathBuf::from("/test/project"));
        manifest.git_commit = Some("abc1234".to_string());
        manifest.git_branch = Some("main".to_string());
        manifest.record_file_hash(PathBuf::from("src/main.rs"), "deadbeef".to_string());
        manifest.record_module(
            "Overview".to_string(),
            "OverviewEditor".to_string(),
            "1.Overview.md".to_string(),
            vec![PathBuf::from("src/main.rs")],
            "# Overview\nSome content",
        );

        let json = serde_json::to_string(&manifest).unwrap();
        let loaded: DocumentationManifest = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.git_commit.as_deref(), Some("abc1234"));
        assert_eq!(loaded.file_hashes.len(), 1);
        assert_eq!(loaded.modules.len(), 1);
        assert!(loaded.modules.contains_key("Overview"));
    }

    #[test]
    fn test_hash_content() {
        let hash = blake3::hash(b"hello world").to_hex().to_string();
        assert_eq!(hash.len(), 64); // BLAKE3 produces 256-bit = 64 hex chars
    }
}
