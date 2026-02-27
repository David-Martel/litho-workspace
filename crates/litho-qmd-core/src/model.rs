use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    Bm25,
    Vector,
    Hybrid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchOptions {
    pub limit: usize,
    pub min_score: f32,
    pub collection: Option<String>,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            limit: 10,
            min_score: 0.0,
            collection: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub docid: String,
    pub file: String,
    pub title: String,
    pub score: f32,
    pub context: Option<String>,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    pub query: String,
    pub mode: SearchMode,
    pub results: Vec<SearchHit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentRequest {
    pub file: String,
    pub from_line: Option<usize>,
    pub max_lines: Option<usize>,
    pub line_numbers: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentContent {
    pub uri: String,
    pub name: String,
    pub title: String,
    pub text: String,
    pub context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiGetRequest {
    pub pattern: String,
    pub max_lines: Option<usize>,
    pub max_bytes: usize,
    pub line_numbers: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiGetResponse {
    pub documents: Vec<DocumentContent>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionRecord {
    pub name: String,
    pub path: String,
    pub pattern: String,
    pub documents: usize,
    pub last_updated: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionMutation {
    pub name: String,
    pub path: String,
    pub pattern: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContextTarget {
    Global,
    CollectionPath { collection: String, path: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextMutation {
    pub target: ContextTarget,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextRecord {
    pub scope: String,
    pub path: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionUpdateResult {
    pub collection: String,
    pub command: String,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stderr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CleanupReport {
    pub llm_cache_rows: usize,
    pub inactive_documents: usize,
    pub orphaned_content: usize,
    pub orphaned_vectors: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStatus {
    pub total_documents: usize,
    pub needs_embedding: usize,
    pub has_vector_index: bool,
    pub collections: Vec<CollectionRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IngestReport {
    pub scanned_files: usize,
    pub indexed_documents: usize,
    pub updated_documents: usize,
    pub unchanged_documents: usize,
    pub deactivated_documents: usize,
    pub failed_files: usize,
    pub duration_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EmbeddingReport {
    pub model: String,
    pub dimension: usize,
    pub embedded: usize,
    pub skipped: usize,
    pub failed: usize,
    pub duration_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelStatus {
    pub name: String,
    pub available: bool,
    pub location: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPullResult {
    pub name: String,
    pub success: bool,
    pub note: String,
}
