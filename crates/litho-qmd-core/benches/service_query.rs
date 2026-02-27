use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use litho_qmd_core::{
    CleanupReport, CollectionMutation, CollectionRecord, CollectionUpdateResult, ContextMutation,
    ContextRecord, ContextTarget, DocumentContent, DocumentRequest, EmbeddingReport, IndexStatus,
    IngestReport, ModelPullResult, ModelStatus, MultiGetRequest, MultiGetResponse, QmdError,
    QmdLlmEngine, QmdResult, QmdService, QmdStore, SearchHit, SearchOptions,
};

#[derive(Clone)]
struct BenchStore {
    lexical: Vec<SearchHit>,
    semantic: Vec<SearchHit>,
}

impl BenchStore {
    fn new(size: usize) -> Self {
        let mut lexical = Vec::with_capacity(size);
        let mut semantic = Vec::with_capacity(size);
        for i in 0..size {
            lexical.push(make_hit(
                format!("doc-{i}"),
                format!("collection/a/file_{i}.md"),
                0.4 + ((size - i) as f32 / size as f32),
            ));
            // Purposefully overlap ~50% docids to exercise hybrid dedup and score replacement.
            let sid = if i % 2 == 0 {
                format!("doc-{i}")
            } else {
                format!("semantic-{i}")
            };
            semantic.push(make_hit(
                sid,
                format!("collection/b/file_{i}.md"),
                0.3 + ((size - i) as f32 / size as f32),
            ));
        }
        Self { lexical, semantic }
    }
}

fn make_hit(docid: String, file: String, score: f32) -> SearchHit {
    SearchHit {
        title: format!("Title {docid}"),
        snippet: "snippet".to_string(),
        context: None,
        docid,
        file,
        score,
    }
}

impl QmdStore for BenchStore {
    fn status(&self) -> QmdResult<IndexStatus> {
        Ok(IndexStatus {
            total_documents: self.lexical.len().max(self.semantic.len()),
            needs_embedding: 0,
            has_vector_index: true,
            collections: vec![],
        })
    }

    fn search_bm25(&self, _query: &str, options: &SearchOptions) -> QmdResult<Vec<SearchHit>> {
        Ok(self
            .lexical
            .iter()
            .take(options.limit * 8)
            .cloned()
            .collect())
    }

    fn search_vector(&self, _query: &str, options: &SearchOptions) -> QmdResult<Vec<SearchHit>> {
        Ok(self
            .semantic
            .iter()
            .take(options.limit * 8)
            .cloned()
            .collect())
    }

    fn get_document(&self, _request: &DocumentRequest) -> QmdResult<DocumentContent> {
        Err(QmdError::NotFound("not used in benchmark".to_string()))
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

#[derive(Clone)]
struct BenchLlm;

impl QmdLlmEngine for BenchLlm {
    fn expand_query(&self, query: &str) -> QmdResult<Vec<String>> {
        Ok(vec![
            query.to_string(),
            format!("{query} rust"),
            format!("{query} architecture"),
        ])
    }

    fn rerank(&self, _query: &str, mut candidates: Vec<SearchHit>) -> QmdResult<Vec<SearchHit>> {
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

fn bench_query(c: &mut Criterion) {
    let mut group = c.benchmark_group("qmd_core_query");

    for corpus in [128usize, 1024usize, 4096usize] {
        let store = BenchStore::new(corpus);
        let service = QmdService::new(store, BenchLlm);
        let opts = SearchOptions {
            limit: 20,
            min_score: 0.0,
            collection: None,
        };

        group.bench_with_input(BenchmarkId::from_parameter(corpus), &corpus, |b, _| {
            b.iter(|| {
                let response = service
                    .query(black_box("ownership borrowing safety"), opts.clone())
                    .expect("query should succeed");
                black_box(response.results.len())
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_query);
criterion_main!(benches);
