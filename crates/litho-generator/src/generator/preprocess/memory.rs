pub struct MemoryScope;

impl MemoryScope {
    pub const PREPROCESS: &'static str = "preprocess";
}

pub struct ScopedKeys;

impl ScopedKeys {
    pub const ORIGINAL_DOCUMENT: &'static str = "original_document";
    pub const PROJECT_STRUCTURE: &'static str = "project_structure";
    pub const CODE_INSIGHTS: &'static str = "code_insights";
    pub const RELATIONSHIPS: &'static str = "relationships";
    pub const INGESTION_DAG: &'static str = "ingestion_dag";
    pub const INGESTION_RAG: &'static str = "ingestion_rag";
}
