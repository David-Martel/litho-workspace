use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Programming language identified for a source file.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Rust,
    TypeScript,
    JavaScript,
    Python,
    CSharp,
    PowerShell,
    Java,
    Go,
    Ruby,
    Cpp,
    C,
    Unknown,
}

/// Coarse role a file plays within a project.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FileClassification {
    EntryPoint,
    Library,
    Module,
    Test,
    Config,
    Documentation,
    BuildScript,
    Migration,
    Unknown,
}

/// Lightweight static-analysis metrics attached to each extracted file.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ComplexityMetrics {
    /// Approximated cyclomatic complexity (McCabe).
    pub cyclomatic: f64,
    /// Non-blank, non-comment lines of code.
    pub lines_of_code: usize,
    /// Count of top-level function / method definitions.
    pub functions: usize,
    /// Count of class / struct / trait definitions.
    pub classes: usize,
}

/// Directed edge in the inter-file dependency graph.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Dependency {
    /// File (or module) that contains the import / use statement.
    pub source: String,
    /// File (or module) being imported.
    pub target: String,
    /// Nature of the dependency (e.g. `"import"`, `"use"`, `"include"`).
    pub kind: String,
}

/// A single exported symbol (function, type, constant, etc.).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Interface {
    /// Symbol name.
    pub name: String,
    /// Symbol kind (e.g. `"fn"`, `"struct"`, `"enum"`, `"trait"`).
    pub kind: String,
    /// Visibility level (e.g. `"pub"`, `"pub(crate)"`, `"private"`).
    pub visibility: String,
    /// Full signature as it appears in the source.
    pub signature: String,
    /// 1-based line number where the symbol is defined.
    pub line: usize,
}

/// Extraction result for a single source file.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExtractedFile {
    /// Project-relative path to the file.
    pub path: PathBuf,
    /// Detected programming language.
    pub language: Language,
    /// Inferred role of the file.
    pub classification: FileClassification,
    /// Static complexity metrics.
    pub complexity: ComplexityMetrics,
    /// Import / use dependencies declared by this file.
    pub dependencies: Vec<Dependency>,
    /// Public symbols exported by this file.
    pub interfaces: Vec<Interface>,
    /// Total lines of code (including comments and blanks).
    pub lines_of_code: usize,
    /// File size in bytes at extraction time.
    pub size_bytes: u64,
}

/// Aggregate statistics computed over the full extracted codebase.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProjectStats {
    /// Total number of files included in the extraction.
    pub total_files: usize,
    /// Sum of `lines_of_code` across all files.
    pub total_loc: usize,
    /// Map from language name to file count.
    pub languages: HashMap<String, usize>,
    /// Paths of the files with the highest cyclomatic complexity.
    pub top_complex_files: Vec<PathBuf>,
}

/// Complete extraction result for a project.
///
/// This is the central data structure passed between [`litho-extract`],
/// [`litho-codex`], and [`litho-cli`].
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExtractedCodebase {
    /// Human-readable project name (derived from the root directory).
    pub project_name: String,
    /// One entry per source file that was successfully extracted.
    pub files: Vec<ExtractedFile>,
    /// Adjacency list: file path → list of file paths it depends on.
    pub dependency_graph: HashMap<String, Vec<String>>,
    /// Rolled-up statistics for the whole codebase.
    pub statistics: ProjectStats,
}

impl ExtractedCodebase {
    /// Returns a one-line human-readable summary.
    ///
    /// # Example
    ///
    /// ```
    /// use litho_core::types::{ExtractedCodebase, ProjectStats};
    /// use std::collections::HashMap;
    ///
    /// let codebase = ExtractedCodebase {
    ///     project_name: "my-project".into(),
    ///     files: vec![],
    ///     dependency_graph: HashMap::new(),
    ///     statistics: ProjectStats {
    ///         total_files: 42,
    ///         total_loc: 8000,
    ///         languages: [("Rust".into(), 42)].into(),
    ///         top_complex_files: vec![],
    ///     },
    /// };
    /// assert_eq!(codebase.summary(), "my-project: 42 files, 8000 LOC, 1 languages");
    /// ```
    pub fn summary(&self) -> String {
        format!(
            "{}: {} files, {} LOC, {} languages",
            self.project_name,
            self.statistics.total_files,
            self.statistics.total_loc,
            self.statistics.languages.len(),
        )
    }
}
