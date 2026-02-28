//! Criterion benchmarks for deterministic, CPU-bound operations in
//! `litho-generator`.
//!
//! Only pure/deterministic functions are benchmarked here — no LLM calls,
//! no I/O, no async.  The benchmarks compile and run entirely offline.
//!
//! Run with:
//! ```text
//! cargo bench -p litho-generator --bench quality_benchmarks
//! ```
//!
//! Verify compilation without running the full suite:
//! ```text
//! cargo bench -p litho-generator --bench quality_benchmarks -- --test
//! ```

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use litho_generator::{
    generator::{
        outlet::md_fixer::MarkdownFixer,
        validator::{ContentValidator, Finding},
    },
    llm::client::ollama_native::parse_json_response,
    types::code::{CodeDossier, CodeInsight},
    utils::token_compress::compress_source_for_llm,
};

// ---------------------------------------------------------------------------
// Synthetic data generators
// ---------------------------------------------------------------------------

/// Generate synthetic Rust source code of approximately `lines` lines.
fn generate_rust_source(lines: usize) -> String {
    let mut source = String::with_capacity(lines * 45);
    source.push_str("// Auto-generated benchmark source\n");
    source.push_str("use std::collections::HashMap;\n\n");
    for i in 0..lines {
        if i % 10 == 0 {
            source.push_str(&format!("/// Documentation for function {i}\n"));
            source.push_str(&format!("pub fn function_{i}(x: i32) -> i32 {{\n"));
            source.push_str(&format!("    // Compute result for {i}\n"));
            source.push_str(&format!("    x + {i}\n"));
            source.push_str("}\n\n");
        } else {
            source.push_str(&format!("    let var_{i} = {i};\n"));
        }
    }
    source
}

/// Generate synthetic markdown documentation with the given number of sections.
fn generate_markdown(sections: usize) -> String {
    let mut md = String::with_capacity(sections * 500);
    md.push_str("# Project Overview\n\n");
    for i in 0..sections {
        md.push_str(&format!("## Section {i}\n\n"));
        md.push_str(&format!(
            "This section describes component {i}. \
             It uses `Config` and `Validator` for processing. \
             The implementation is in `src/module_{i}.rs` \
             and handles both sync and async operations.\n\n"
        ));
        md.push_str(&format!("### Subsection {i}.1\n\n"));
        md.push_str("Details about the internal architecture and data flow.\n\n");
        if i % 3 == 0 {
            md.push_str("```mermaid\ngraph TD\n    A --> B\n    B --> C\n```\n\n");
        }
    }
    md
}

/// Generate `count` doc sections suitable for validator benchmarks.
///
/// The first four entries always match the required C4 section names so that
/// completeness scoring reaches 1.0 for those benchmarks.
fn generate_doc_sections(count: usize) -> Vec<(String, String)> {
    let mut sections = vec![
        ("Project Overview".to_string(), "x".repeat(500)),
        ("Architecture Description".to_string(), "x".repeat(500)),
        ("Core Workflows".to_string(), "x".repeat(500)),
        ("Boundary Interfaces".to_string(), "x".repeat(500)),
    ];
    for i in 0..count.saturating_sub(4) {
        sections.push((
            format!("Extra Section {i}"),
            format!(
                "Content for section {i} with `Config` and `execute()` and `validate()` references."
            ),
        ));
    }
    sections
}

// ---------------------------------------------------------------------------
// token_compress benchmarks
// ---------------------------------------------------------------------------

fn bench_token_compress(c: &mut Criterion) {
    let mut group = c.benchmark_group("token_compress");

    for &size in &[100usize, 500, 1000, 5000] {
        let source = generate_rust_source(size);
        group.bench_with_input(BenchmarkId::new("rust_lines", size), &source, |b, src| {
            b.iter(|| compress_source_for_llm(black_box(src), Some("rs")))
        });
    }

    // Python path (different comment-stripping logic)
    let py_source = generate_rust_source(500)
        .lines()
        .map(|l| l.replace("//", "#"))
        .collect::<Vec<_>>()
        .join("\n");
    group.bench_with_input(
        BenchmarkId::new("python_lines", 500),
        &py_source,
        |b, src| b.iter(|| compress_source_for_llm(black_box(src), Some("py"))),
    );

    group.finish();
}

// ---------------------------------------------------------------------------
// markdown_fixer benchmarks
// ---------------------------------------------------------------------------

fn bench_markdown_fixer(c: &mut Criterion) {
    let mut group = c.benchmark_group("markdown_fixer");

    for &sections in &[5usize, 20, 50] {
        let md = generate_markdown(sections);
        group.bench_with_input(BenchmarkId::new("sections", sections), &md, |b, input| {
            b.iter(|| MarkdownFixer::fix(black_box(input)))
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// json_parse_cascade benchmarks
// ---------------------------------------------------------------------------

fn bench_json_parse_cascade(c: &mut Criterion) {
    let mut group = c.benchmark_group("json_parse_cascade");

    // Strategy 1: direct JSON — fastest path.
    let direct = r#"{"name":"test","value":42,"items":["a","b","c"]}"#;
    group.bench_function("direct_json", |b| {
        b.iter(|| parse_json_response(black_box(direct), 1))
    });

    // Strategy 2: JSON inside a fenced code block.
    let code_block = "Here is the result:\n```json\n{\"name\":\"test\",\"value\":42}\n```\nDone.";
    group.bench_function("code_block", |b| {
        b.iter(|| parse_json_response(black_box(code_block), 1))
    });

    // Strategy 3: JSON buried in surrounding prose.
    let buried = "Thinking... The answer is {\"x\":1,\"y\":2} and that is the end.";
    group.bench_function("buried_object", |b| {
        b.iter(|| parse_json_response(black_box(buried), 1))
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// quality / completeness benchmarks
// ---------------------------------------------------------------------------

fn bench_completeness_check(c: &mut Criterion) {
    let mut group = c.benchmark_group("quality_completeness");

    for &count in &[4usize, 10, 20] {
        let sections = generate_doc_sections(count);
        group.bench_with_input(BenchmarkId::new("sections", count), &sections, |b, secs| {
            b.iter(|| {
                let mut findings: Vec<Finding> = Vec::new();
                ContentValidator::check_completeness(black_box(secs), &mut findings)
            })
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// quality / coherence benchmarks
// ---------------------------------------------------------------------------

fn bench_coherence_check(c: &mut Criterion) {
    let mut group = c.benchmark_group("quality_coherence");

    for &count in &[4usize, 10, 20] {
        let sections = generate_doc_sections(count);
        group.bench_with_input(BenchmarkId::new("sections", count), &sections, |b, secs| {
            b.iter(|| {
                let mut findings: Vec<Finding> = Vec::new();
                ContentValidator::check_coherence(black_box(secs), &mut findings)
            })
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// quality / symbol-coverage benchmarks
// ---------------------------------------------------------------------------

fn bench_symbol_coverage(c: &mut Criterion) {
    let mut group = c.benchmark_group("quality_symbol_coverage");

    // Build synthetic code insights: 50 files × 5 functions + 1 struct = 300 symbols.
    let insights: Vec<CodeInsight> = (0..50)
        .map(|i| CodeInsight {
            code_dossier: CodeDossier {
                name: format!("file_{i}.rs"),
                functions: (0..5).map(|j| format!("function_{i}_{j}")).collect(),
                interfaces: vec![format!("Struct_{i}")],
                ..Default::default()
            },
            ..Default::default()
        })
        .collect();

    let sections = generate_doc_sections(10);

    group.bench_function("50_files_10_sections", |b| {
        b.iter(|| {
            let mut findings: Vec<Finding> = Vec::new();
            ContentValidator::check_symbol_coverage(
                black_box(&sections),
                Some(black_box(&insights)),
                &mut findings,
            )
        })
    });

    // Also benchmark the "no insights" fast-path.
    group.bench_function("no_insights_fast_path", |b| {
        b.iter(|| {
            let mut findings: Vec<Finding> = Vec::new();
            ContentValidator::check_symbol_coverage(black_box(&sections), None, &mut findings)
        })
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Criterion harness
// ---------------------------------------------------------------------------

criterion_group!(
    benches,
    bench_token_compress,
    bench_markdown_fixer,
    bench_json_parse_cascade,
    bench_completeness_check,
    bench_coherence_check,
    bench_symbol_coverage,
);
criterion_main!(benches);
