pub mod code;
pub mod code_releationship;
pub mod original_document;
pub mod project_structure;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileInfo {
    pub path: PathBuf,
    #[serde(default)]
    pub name: String,
    pub size: u64,
    #[serde(default)]
    pub extension: Option<String>,
    pub is_core: bool,
    pub importance_score: f64,
    pub complexity_score: f64,
    #[serde(default)]
    pub last_modified: Option<String>,
}

/// Directory information
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DirectoryInfo {
    pub path: PathBuf,
    #[serde(default)]
    pub name: String,
    pub file_count: usize,
    pub subdirectory_count: usize,
    pub total_size: u64,
    pub importance_score: f64,
}
