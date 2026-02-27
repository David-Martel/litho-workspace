use async_trait::async_trait;
use litho_core::types::ExtractedCodebase;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// A single section of generated documentation.
///
/// # Example
///
/// ```
/// use litho_codex::provider::DocumentSection;
///
/// let section = DocumentSection {
///     title: "Architecture Overview".into(),
///     content: "## Overview\n\nThis project...".into(),
///     filename: "architecture.md".into(),
/// };
/// assert_eq!(section.filename, "architecture.md");
/// ```
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DocumentSection {
    /// Human-readable heading for this section.
    pub title: String,
    /// Full Markdown content of the section.
    pub content: String,
    /// Suggested output filename (e.g. `"architecture.md"`).
    pub filename: String,
}

/// Trait implemented by any documentation generator backend.
///
/// The primary implementation is [`crate::exec::CodexExecGenerator`], which
/// invokes `codex exec` as a subprocess. Alternative implementations (e.g.
/// direct API clients) can be swapped in for testing or alternative providers.
#[async_trait]
pub trait DocGenerator: Send + Sync {
    /// Generate documentation sections from an extracted codebase.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying generator process fails, if
    /// required output files cannot be read, or if any I/O error occurs.
    async fn generate(
        &self,
        extracted: &ExtractedCodebase,
        project_path: &Path,
        output_path: &Path,
    ) -> anyhow::Result<Vec<DocumentSection>>;

    /// Checks if the generator is ready to be used (e.g. required binaries are found).
    ///
    /// # Errors
    ///
    /// Returns an error describing why the generator is not ready.
    async fn validate_readiness(&self) -> anyhow::Result<()>;
}
