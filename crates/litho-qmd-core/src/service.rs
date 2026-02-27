use crate::error::{QmdError, QmdResult};
use crate::model::{
    CleanupReport, CollectionMutation, CollectionRecord, CollectionUpdateResult, ContextMutation,
    ContextRecord, ContextTarget, DocumentContent, DocumentRequest, EmbeddingReport, IndexStatus,
    IngestReport, ModelPullResult, ModelStatus, MultiGetRequest, MultiGetResponse, SearchHit,
    SearchMode, SearchOptions, SearchResponse,
};
use crate::traits::{QmdLlmEngine, QmdStore};

pub struct QmdService<S, L> {
    pub store: S,
    pub llm: L,
}

impl<S, L> QmdService<S, L>
where
    S: QmdStore,
    L: QmdLlmEngine,
{
    pub fn new(store: S, llm: L) -> Self {
        Self { store, llm }
    }

    pub fn search(&self, query: &str, options: SearchOptions) -> QmdResult<SearchResponse> {
        let mut results = self.store.search_bm25(query, &options)?;
        results.retain(|hit| hit.score >= options.min_score);
        Ok(SearchResponse {
            query: query.to_string(),
            mode: SearchMode::Bm25,
            results,
        })
    }

    pub fn vsearch(&self, query: &str, options: SearchOptions) -> QmdResult<SearchResponse> {
        let mut results = self.store.search_vector(query, &options)?;
        results.retain(|hit| hit.score >= options.min_score);
        Ok(SearchResponse {
            query: query.to_string(),
            mode: SearchMode::Vector,
            results,
        })
    }

    pub fn query(&self, query: &str, options: SearchOptions) -> QmdResult<SearchResponse> {
        let expanded = self.llm.expand_query(query)?;
        if expanded.is_empty() {
            return Err(QmdError::Internal(
                "llm returned zero query variants".to_string(),
            ));
        }

        let mut merged: Vec<SearchHit> = Vec::new();
        for variant in expanded {
            let mut batch = self.store.search_bm25(&variant, &options)?;
            merged.append(&mut batch);
            let mut semantic = self
                .store
                .search_vector(&variant, &options)
                .unwrap_or_default();
            merged.append(&mut semantic);
        }

        merged = dedup_hits(merged);
        let mut reranked = self.llm.rerank(query, merged)?;
        reranked.retain(|hit| hit.score >= options.min_score);
        reranked.truncate(options.limit);

        Ok(SearchResponse {
            query: query.to_string(),
            mode: SearchMode::Hybrid,
            results: reranked,
        })
    }

    pub fn get(&self, request: &DocumentRequest) -> QmdResult<DocumentContent> {
        self.store.get_document(request)
    }

    pub fn multi_get(&self, request: &MultiGetRequest) -> QmdResult<MultiGetResponse> {
        self.store.multi_get(request)
    }

    pub fn list_files(&self, prefix: Option<&str>) -> QmdResult<Vec<String>> {
        self.store.list_files(prefix)
    }

    pub fn status(&self) -> QmdResult<IndexStatus> {
        self.store.status()
    }

    pub fn list_collections(&self) -> QmdResult<Vec<CollectionRecord>> {
        self.store.list_collections()
    }

    pub fn add_collection(&self, input: &CollectionMutation) -> QmdResult<()> {
        self.store.add_collection(input)
    }

    pub fn remove_collection(&self, name: &str) -> QmdResult<bool> {
        self.store.remove_collection(name)
    }

    pub fn rename_collection(&self, old_name: &str, new_name: &str) -> QmdResult<()> {
        self.store.rename_collection(old_name, new_name)
    }

    pub fn run_updates(&self) -> QmdResult<Vec<CollectionUpdateResult>> {
        self.store.run_collection_updates()
    }

    pub fn ingest(&self, force: bool) -> QmdResult<IngestReport> {
        self.store.ingest_collections(force)
    }

    pub fn embed(&self, force: bool) -> QmdResult<EmbeddingReport> {
        self.store.embed_native(force)
    }

    pub fn list_contexts(&self) -> QmdResult<Vec<ContextRecord>> {
        self.store.list_contexts()
    }

    pub fn add_context(&self, input: &ContextMutation) -> QmdResult<()> {
        self.store.add_context(input)
    }

    pub fn remove_context(&self, target: &ContextTarget) -> QmdResult<bool> {
        self.store.remove_context(target)
    }

    pub fn cleanup(&self) -> QmdResult<CleanupReport> {
        self.store.cleanup()
    }

    pub fn local_models(&self) -> QmdResult<Vec<ModelStatus>> {
        self.llm.local_models()
    }

    pub fn pull_models(&self, refresh: bool) -> QmdResult<Vec<ModelPullResult>> {
        self.llm.pull_models(refresh)
    }
}

fn dedup_hits(hits: Vec<SearchHit>) -> Vec<SearchHit> {
    use std::collections::BTreeMap;
    let mut by_docid = BTreeMap::<String, SearchHit>::new();
    for hit in hits {
        let key = if hit.docid.is_empty() {
            hit.file.clone()
        } else {
            hit.docid.clone()
        };
        match by_docid.get(&key) {
            Some(existing) if existing.score >= hit.score => {}
            _ => {
                by_docid.insert(key, hit);
            }
        }
    }
    let mut out = by_docid.into_values().collect::<Vec<_>>();
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DocumentRequest, MultiGetRequest};
    use crate::traits::{QmdLlmEngine, QmdStore};

    #[derive(Clone)]
    struct MockStore {
        lexical: Vec<SearchHit>,
        semantic: Vec<SearchHit>,
    }

    #[derive(Clone)]
    struct MockLlm {
        variants: Vec<String>,
    }

    fn hit(docid: &str, score: f32) -> SearchHit {
        SearchHit {
            docid: docid.to_string(),
            file: format!("docs/{docid}.md"),
            title: format!("Title {docid}"),
            score,
            context: None,
            snippet: "snippet".to_string(),
        }
    }

    impl QmdStore for MockStore {
        fn status(&self) -> QmdResult<IndexStatus> {
            Ok(IndexStatus {
                total_documents: 0,
                needs_embedding: 0,
                has_vector_index: true,
                collections: vec![],
            })
        }

        fn search_bm25(&self, _query: &str, _options: &SearchOptions) -> QmdResult<Vec<SearchHit>> {
            Ok(self.lexical.clone())
        }

        fn search_vector(
            &self,
            _query: &str,
            _options: &SearchOptions,
        ) -> QmdResult<Vec<SearchHit>> {
            Ok(self.semantic.clone())
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

        fn list_collections(&self) -> QmdResult<Vec<CollectionRecord>> {
            Ok(vec![])
        }

        fn add_collection(&self, _input: &CollectionMutation) -> QmdResult<()> {
            Ok(())
        }

        fn remove_collection(&self, _name: &str) -> QmdResult<bool> {
            Ok(false)
        }

        fn rename_collection(&self, _old_name: &str, _new_name: &str) -> QmdResult<()> {
            Ok(())
        }

        fn run_collection_updates(&self) -> QmdResult<Vec<CollectionUpdateResult>> {
            Ok(vec![])
        }

        fn ingest_collections(&self, _force: bool) -> QmdResult<IngestReport> {
            Ok(IngestReport::default())
        }

        fn embed_native(&self, _force: bool) -> QmdResult<EmbeddingReport> {
            Ok(EmbeddingReport::default())
        }

        fn list_contexts(&self) -> QmdResult<Vec<ContextRecord>> {
            Ok(vec![])
        }

        fn add_context(&self, _input: &ContextMutation) -> QmdResult<()> {
            Ok(())
        }

        fn remove_context(&self, _target: &ContextTarget) -> QmdResult<bool> {
            Ok(false)
        }

        fn cleanup(&self) -> QmdResult<CleanupReport> {
            Ok(CleanupReport::default())
        }
    }

    impl QmdLlmEngine for MockLlm {
        fn expand_query(&self, _query: &str) -> QmdResult<Vec<String>> {
            Ok(self.variants.clone())
        }

        fn rerank(
            &self,
            _query: &str,
            mut candidates: Vec<SearchHit>,
        ) -> QmdResult<Vec<SearchHit>> {
            candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
            Ok(candidates)
        }

        fn local_models(&self) -> QmdResult<Vec<ModelStatus>> {
            Ok(vec![])
        }

        fn pull_models(&self, _refresh: bool) -> QmdResult<Vec<ModelPullResult>> {
            Ok(vec![])
        }
    }

    #[test]
    fn query_deduplicates_by_docid_and_keeps_highest_score() {
        let store = MockStore {
            lexical: vec![hit("a", 0.50), hit("b", 0.70), hit("x", 0.40)],
            semantic: vec![hit("a", 0.90), hit("c", 0.80), hit("x", 0.20)],
        };
        let llm = MockLlm {
            variants: vec!["q".to_string()],
        };
        let service = QmdService::new(store, llm);

        let result = service
            .query(
                "q",
                SearchOptions {
                    limit: 10,
                    min_score: 0.0,
                    collection: None,
                },
            )
            .expect("query should work");

        let a = result
            .results
            .iter()
            .find(|h| h.docid == "a")
            .expect("doc a should exist");
        assert_eq!(a.score, 0.90);
        assert_eq!(result.mode, SearchMode::Hybrid);
    }

    #[test]
    fn query_enforces_limit_and_min_score_after_rerank() {
        let store = MockStore {
            lexical: vec![hit("a", 0.95), hit("b", 0.60), hit("c", 0.20)],
            semantic: vec![hit("d", 0.85), hit("e", 0.75), hit("b", 0.65)],
        };
        let llm = MockLlm {
            variants: vec!["q".to_string(), "q2".to_string()],
        };
        let service = QmdService::new(store, llm);

        let result = service
            .query(
                "q",
                SearchOptions {
                    limit: 3,
                    min_score: 0.70,
                    collection: None,
                },
            )
            .expect("query should work");

        assert!(result.results.len() <= 3);
        assert!(result.results.iter().all(|h| h.score >= 0.70));
    }

    #[test]
    fn query_errors_when_llm_returns_zero_variants() {
        let store = MockStore {
            lexical: vec![],
            semantic: vec![],
        };
        let llm = MockLlm { variants: vec![] };
        let service = QmdService::new(store, llm);

        let err = service
            .query(
                "q",
                SearchOptions {
                    limit: 5,
                    min_score: 0.0,
                    collection: None,
                },
            )
            .expect_err("empty expansion should fail");
        assert!(format!("{err}").contains("zero query variants"));
    }
}
