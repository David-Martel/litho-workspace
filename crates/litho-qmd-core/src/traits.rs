use crate::error::QmdResult;
use crate::model::{
    CleanupReport, CollectionMutation, CollectionRecord, CollectionUpdateResult, ContextMutation,
    ContextRecord, ContextTarget, DocumentContent, DocumentRequest, EmbeddingReport, IndexStatus,
    IngestReport, ModelPullResult, ModelStatus, MultiGetRequest, MultiGetResponse, SearchHit,
    SearchOptions,
};

pub trait QmdStore {
    fn status(&self) -> QmdResult<IndexStatus>;
    fn search_bm25(&self, query: &str, options: &SearchOptions) -> QmdResult<Vec<SearchHit>>;
    fn search_vector(&self, _query: &str, _options: &SearchOptions) -> QmdResult<Vec<SearchHit>>;
    fn get_document(&self, request: &DocumentRequest) -> QmdResult<DocumentContent>;
    fn multi_get(&self, request: &MultiGetRequest) -> QmdResult<MultiGetResponse>;
    fn list_files(&self, prefix: Option<&str>) -> QmdResult<Vec<String>>;

    fn list_collections(&self) -> QmdResult<Vec<CollectionRecord>>;
    fn add_collection(&self, input: &CollectionMutation) -> QmdResult<()>;
    fn remove_collection(&self, name: &str) -> QmdResult<bool>;
    fn rename_collection(&self, old_name: &str, new_name: &str) -> QmdResult<()>;
    fn run_collection_updates(&self) -> QmdResult<Vec<CollectionUpdateResult>>;
    fn ingest_collections(&self, force: bool) -> QmdResult<IngestReport>;
    fn embed_native(&self, force: bool) -> QmdResult<EmbeddingReport>;

    fn list_contexts(&self) -> QmdResult<Vec<ContextRecord>>;
    fn add_context(&self, input: &ContextMutation) -> QmdResult<()>;
    fn remove_context(&self, target: &ContextTarget) -> QmdResult<bool>;

    fn cleanup(&self) -> QmdResult<CleanupReport>;
}

pub trait QmdLlmEngine {
    fn expand_query(&self, query: &str) -> QmdResult<Vec<String>>;
    fn rerank(&self, query: &str, candidates: Vec<SearchHit>) -> QmdResult<Vec<SearchHit>>;
    fn local_models(&self) -> QmdResult<Vec<ModelStatus>>;
    fn pull_models(&self, refresh: bool) -> QmdResult<Vec<ModelPullResult>>;
}
