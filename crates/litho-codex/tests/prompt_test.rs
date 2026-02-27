use litho_codex::prompts::{RetrievedSnippet, append_qmd_retrieval_context, build_prompt};
use litho_core::types::{ExtractedCodebase, ProjectStats};
use std::collections::HashMap;

fn make_codebase(
    name: &str,
    total_files: usize,
    languages: HashMap<String, usize>,
) -> ExtractedCodebase {
    ExtractedCodebase {
        project_name: name.into(),
        files: vec![],
        dependency_graph: HashMap::new(),
        statistics: ProjectStats {
            total_files,
            total_loc: 5000,
            languages,
            top_complex_files: vec![],
        },
    }
}

#[test]
fn prompt_includes_project_stats() {
    let codebase = ExtractedCodebase {
        project_name: "test-project".into(),
        files: vec![],
        dependency_graph: Default::default(),
        statistics: ProjectStats {
            total_files: 42,
            total_loc: 5000,
            languages: [("Rust".into(), 30), ("Python".into(), 12)].into(),
            top_complex_files: vec![],
        },
    };

    let prompt = build_prompt(&codebase, "architecture");

    assert!(
        prompt.contains("test-project"),
        "prompt must contain the project name"
    );
    assert!(
        prompt.contains("42 files"),
        "prompt must mention the total file count"
    );
    assert!(
        prompt.contains("Rust"),
        "prompt must list the Rust language entry"
    );
}

#[test]
fn prompt_includes_python_language() {
    let codebase = make_codebase("python-proj", 10, [("Python".into(), 10)].into());
    let prompt = build_prompt(&codebase, "full");
    assert!(
        prompt.contains("Python"),
        "prompt must include Python language"
    );
    assert!(
        prompt.contains("10 files"),
        "prompt must show correct file count"
    );
}

#[test]
fn prompt_contains_section_hint_for_architecture() {
    let codebase = make_codebase("arch-proj", 5, HashMap::new());
    let prompt = build_prompt(&codebase, "architecture");
    // The architecture section hint text appears in the prompt
    assert!(
        prompt.contains("Architecture"),
        "prompt must reference the Architecture section"
    );
}

#[test]
fn prompt_contains_section_hint_for_full() {
    let codebase = make_codebase("full-proj", 5, HashMap::new());
    let prompt = build_prompt(&codebase, "full");
    assert!(
        prompt.contains("all five sections"),
        "full section hint must be present"
    );
}

#[test]
fn prompt_handles_empty_files_gracefully() {
    let codebase = make_codebase("empty-proj", 0, HashMap::new());
    let prompt = build_prompt(&codebase, "full");
    // Should not panic and should still include project name
    assert!(prompt.contains("empty-proj"));
    assert!(prompt.contains("no source files extracted"));
    assert!(prompt.contains("no public interfaces found"));
}

#[test]
fn prompt_includes_loc() {
    let codebase = ExtractedCodebase {
        project_name: "loc-proj".into(),
        files: vec![],
        dependency_graph: HashMap::new(),
        statistics: ProjectStats {
            total_files: 1,
            total_loc: 9999,
            languages: HashMap::new(),
            top_complex_files: vec![],
        },
    };
    let prompt = build_prompt(&codebase, "full");
    assert!(prompt.contains("9999"), "prompt must include total LOC");
}

#[test]
fn prompt_append_qmd_retrieval_context_includes_citations() {
    let base = "# Base Prompt".to_string();
    let snippets = vec![RetrievedSnippet {
        file: "crates/litho-book/src/server.rs".to_string(),
        title: "Search Handler".to_string(),
        snippet: "Handles /api/search with in-memory and qmd fallback.".to_string(),
        score: 0.91,
    }];

    let out = append_qmd_retrieval_context(base, &snippets);
    assert!(out.contains("Retrieved Evidence (QMD)"));
    assert!(out.contains("crates/litho-book/src/server.rs"));
    assert!(out.contains("Search Handler"));
}

#[test]
fn prompt_append_qmd_retrieval_context_noop_when_empty() {
    let base = "# Base Prompt".to_string();
    let out = append_qmd_retrieval_context(base.clone(), &[]);
    assert_eq!(out, base);
}
