use litho_qmd_core::model::{DocumentContent, DocumentRequest, MultiGetRequest, MultiGetResponse};
use litho_qmd_core::{
    QmdError, QmdLlmEngine, QmdResult, QmdService, QmdStore, SearchHit, SearchMode, SearchOptions,
    SearchResponse,
};

#[derive(Clone, Default)]
struct StubStore;

#[derive(Clone, Default)]
struct StubLlm;

impl QmdStore for StubStore {
    fn status(&self) -> QmdResult<litho_qmd_core::IndexStatus> {
        Ok(litho_qmd_core::IndexStatus {
            total_documents: 0,
            needs_embedding: 0,
            has_vector_index: false,
            collections: vec![],
        })
    }

    fn search_bm25(&self, _query: &str, _options: &SearchOptions) -> QmdResult<Vec<SearchHit>> {
        Ok(vec![SearchHit {
            docid: "doc-1".to_string(),
            file: "docs/doc-1.md".to_string(),
            title: "Doc 1".to_string(),
            score: 0.9,
            context: None,
            snippet: "snippet".to_string(),
        }])
    }

    fn search_vector(&self, _query: &str, _options: &SearchOptions) -> QmdResult<Vec<SearchHit>> {
        Ok(vec![])
    }

    fn get_document(&self, _request: &DocumentRequest) -> QmdResult<DocumentContent> {
        Err(QmdError::NotFound("unused".to_string()))
    }

    fn multi_get(&self, _request: &MultiGetRequest) -> QmdResult<MultiGetResponse> {
        Ok(MultiGetResponse {
            documents: vec![],
            errors: vec![],
        })
    }

    fn list_files(&self, _prefix: Option<&str>) -> QmdResult<Vec<String>> {
        Ok(vec![])
    }

    fn list_collections(&self) -> QmdResult<Vec<litho_qmd_core::CollectionRecord>> {
        Ok(vec![])
    }

    fn add_collection(&self, _input: &litho_qmd_core::CollectionMutation) -> QmdResult<()> {
        Ok(())
    }

    fn remove_collection(&self, _name: &str) -> QmdResult<bool> {
        Ok(false)
    }

    fn rename_collection(&self, _old_name: &str, _new_name: &str) -> QmdResult<()> {
        Ok(())
    }

    fn run_collection_updates(&self) -> QmdResult<Vec<litho_qmd_core::CollectionUpdateResult>> {
        Ok(vec![])
    }

    fn ingest_collections(&self, _force: bool) -> QmdResult<litho_qmd_core::IngestReport> {
        Ok(litho_qmd_core::IngestReport::default())
    }

    fn embed_native(&self, _force: bool) -> QmdResult<litho_qmd_core::EmbeddingReport> {
        Ok(litho_qmd_core::EmbeddingReport::default())
    }

    fn list_contexts(&self) -> QmdResult<Vec<litho_qmd_core::ContextRecord>> {
        Ok(vec![])
    }

    fn add_context(&self, _input: &litho_qmd_core::ContextMutation) -> QmdResult<()> {
        Ok(())
    }

    fn remove_context(&self, _target: &litho_qmd_core::ContextTarget) -> QmdResult<bool> {
        Ok(false)
    }

    fn cleanup(&self) -> QmdResult<litho_qmd_core::CleanupReport> {
        Ok(litho_qmd_core::CleanupReport::default())
    }
}

impl QmdLlmEngine for StubLlm {
    fn expand_query(&self, query: &str) -> QmdResult<Vec<String>> {
        Ok(vec![query.to_string()])
    }

    fn rerank(&self, _query: &str, candidates: Vec<SearchHit>) -> QmdResult<Vec<SearchHit>> {
        Ok(candidates)
    }

    fn local_models(&self) -> QmdResult<Vec<litho_qmd_core::ModelStatus>> {
        Ok(vec![])
    }

    fn pull_models(&self, _refresh: bool) -> QmdResult<Vec<litho_qmd_core::ModelPullResult>> {
        Ok(vec![])
    }
}

#[test]
fn legacy_core_exports_compile_and_execute() {
    let service = QmdService::new(StubStore, StubLlm);

    let response = service
        .search("doc", SearchOptions::default())
        .expect("legacy service should remain callable");

    assert_eq!(response.mode, SearchMode::Bm25);
    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].docid, "doc-1");
}

#[test]
fn legacy_core_module_paths_remain_available() {
    let _from_module: litho_qmd_core::service::QmdService<StubStore, StubLlm> =
        QmdService::new(StubStore, StubLlm);

    let response = SearchResponse {
        query: "q".to_string(),
        mode: SearchMode::Hybrid,
        results: vec![],
    };
    assert!(response.results.is_empty());
}
