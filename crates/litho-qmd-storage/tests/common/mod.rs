use litho_qmd_storage::{ModelPullResult, ModelStatus, QmdLlmEngine, QmdResult, SearchHit};

#[derive(Debug, Clone, Default)]
pub struct NoopTestLlmEngine;

impl QmdLlmEngine for NoopTestLlmEngine {
    fn expand_query(&self, query: &str) -> QmdResult<Vec<String>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(vec![]);
        }
        Ok(vec![query.to_string()])
    }

    fn rerank(&self, _query: &str, candidates: Vec<SearchHit>) -> QmdResult<Vec<SearchHit>> {
        Ok(candidates)
    }

    fn local_models(&self) -> QmdResult<Vec<ModelStatus>> {
        Ok(vec![])
    }

    fn pull_models(&self, _refresh: bool) -> QmdResult<Vec<ModelPullResult>> {
        Ok(vec![])
    }
}
