use litho_qmd_llm::{AdaptiveLlmEngine, NoopLlmEngine};
use litho_qmd_storage::{QmdLlmEngine, SearchHit};

#[test]
fn legacy_llm_exports_compile() {
    let noop = NoopLlmEngine;
    let variants = noop
        .expand_query("Rust Config Build")
        .expect("noop expand_query should succeed");
    assert!(!variants.is_empty());

    let adaptive = AdaptiveLlmEngine::from_env();
    let reranked = adaptive
        .rerank(
            "query",
            vec![SearchHit {
                docid: "doc-1".to_string(),
                file: "docs/doc-1.md".to_string(),
                title: "Doc 1".to_string(),
                score: 0.75,
                context: None,
                snippet: "snippet".to_string(),
            }],
        )
        .expect("adaptive rerank should remain callable");
    assert_eq!(reranked.len(), 1);
}
