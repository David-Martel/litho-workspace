use litho_core::types::ExtractedCodebase;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

/// Build a structured prompt for the given `section` of documentation.
///
/// The prompt includes project statistics, key files ranked by complexity, and
/// the top 100 public interfaces — giving the LLM enough context to produce
/// accurate C4-model architecture documentation without reading every file.
///
/// # Arguments
///
/// * `codebase` — The fully-extracted project snapshot.
/// * `section`  — A hint about which documentation section to emphasise
///   (e.g. `"architecture"`, `"full"`).
///
/// # Example
///
/// ```
/// use litho_codex::prompts::build_prompt;
/// use litho_core::types::{ExtractedCodebase, ProjectStats};
/// use std::collections::HashMap;
///
/// let codebase = ExtractedCodebase {
///     project_name: "demo".into(),
///     files: vec![],
///     dependency_graph: HashMap::new(),
///     statistics: ProjectStats {
///         total_files: 1,
///         total_loc: 100,
///         languages: [("Rust".into(), 1)].into(),
///         top_complex_files: vec![],
///     },
/// };
/// let prompt = build_prompt(&codebase, "architecture");
/// assert!(prompt.contains("demo"));
/// assert!(prompt.contains("1 files"));
/// ```
pub fn build_prompt(codebase: &ExtractedCodebase, section: &str) -> String {
    let stats = &codebase.statistics;

    let lang_summary: Vec<String> = stats
        .languages
        .iter()
        .map(|(lang, count)| format!("{lang}: {count} files"))
        .collect();

    let top_files: Vec<String> = codebase
        .files
        .iter()
        .take(20)
        .map(|f| {
            format!(
                "  - {} ({:?}, {} LOC, complexity={:.0})",
                f.path.display(),
                f.classification,
                f.lines_of_code,
                f.complexity.cyclomatic
            )
        })
        .collect();

    let interfaces: Vec<String> = codebase
        .files
        .iter()
        .flat_map(|f| {
            f.interfaces
                .iter()
                .filter(|i| i.visibility == "pub")
                .map(move |i| {
                    format!(
                        "  - {}:{} {} {} {}",
                        f.path.display(),
                        i.line,
                        i.kind,
                        i.name,
                        i.signature
                    )
                })
        })
        .take(100)
        .collect();

    let section_hint = match section {
        "architecture" => "Focus especially on the **Architecture** and **Boundaries** sections.",
        "full" => "Produce all five sections in full detail.",
        other => &format!("Emphasise the '{other}' section."),
    };

    format!(
        r#"You are analyzing the "{name}" codebase to produce C4-model architecture documentation.

## Project Summary
- {total_files} files, {total_loc} lines of code
- Languages: {langs}

## Key Files (by complexity)
{files}

## Public Interfaces (top 100)
{interfaces}

## Task
{section_hint}

Produce comprehensive architecture documentation in Markdown covering:
1. **Overview** — Purpose, stakeholders, system context
2. **Architecture** — Component/container diagram, design patterns, key decisions
3. **Workflows** — Primary data/control flows, sequence diagrams (Mermaid)
4. **Boundaries** — External interfaces, API contracts, integration points
5. **Database** — Data model, storage patterns (if applicable)

Read specific source files as needed for deeper analysis.
Output each section with a ## heading. Use Mermaid diagrams where helpful.
"#,
        name = codebase.project_name,
        total_files = stats.total_files,
        total_loc = stats.total_loc,
        langs = lang_summary.join(", "),
        files = if top_files.is_empty() {
            "  (no source files extracted)".into()
        } else {
            top_files.join("\n")
        },
        interfaces = if interfaces.is_empty() {
            "  (no public interfaces found)".into()
        } else {
            interfaces.join("\n")
        },
    )
}

#[derive(Debug, Clone, Deserialize)]
pub struct RetrievedSnippet {
    pub file: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub snippet: String,
    #[serde(default)]
    pub score: f32,
}

#[derive(Debug, Deserialize)]
struct RetrievedSearchResponse {
    #[serde(default)]
    results: Vec<RetrievedSnippet>,
}

pub fn build_prompt_with_optional_qmd(
    codebase: &ExtractedCodebase,
    section: &str,
    project_path: &Path,
) -> String {
    let base = build_prompt(codebase, section);
    let snippets = maybe_qmd_retrieval_snippets(codebase, section, project_path);
    append_qmd_retrieval_context(base, &snippets)
}

pub fn append_qmd_retrieval_context(base_prompt: String, snippets: &[RetrievedSnippet]) -> String {
    if snippets.is_empty() {
        return base_prompt;
    }

    let mut extra = String::from(
        "\n\n## Retrieved Evidence (QMD)\nUse these citations when relevant; do not invent sources.\n",
    );
    for snippet in snippets.iter().take(8) {
        let title = if snippet.title.trim().is_empty() {
            "(untitled)".to_string()
        } else {
            snippet.title.trim().to_string()
        };
        let condensed = snippet
            .snippet
            .replace('\n', " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let clipped = if condensed.len() > 220 {
            format!("{}...", &condensed[..220])
        } else {
            condensed
        };
        extra.push_str(&format!(
            "- `{}` ({title}, score {:.3}): {clipped}\n",
            snippet.file, snippet.score
        ));
    }

    format!("{base_prompt}{extra}")
}

fn maybe_qmd_retrieval_snippets(
    codebase: &ExtractedCodebase,
    section: &str,
    project_path: &Path,
) -> Vec<RetrievedSnippet> {
    if !is_truthy(std::env::var("LITHO_CODEX_QMD_AUGMENT").ok().as_deref()) {
        return Vec::new();
    }

    if let Some(mock_json) = std::env::var("LITHO_CODEX_QMD_SNIPPETS_JSON").ok()
        && let Ok(parsed) = serde_json::from_str::<RetrievedSearchResponse>(&mock_json)
    {
        return dedup_snippets(parsed.results);
    }

    let bin = std::env::var("LITHO_CODEX_QMD_BIN").unwrap_or_else(|_| "qmd".to_string());
    let mode = std::env::var("LITHO_CODEX_QMD_MODE").unwrap_or_else(|_| "query".to_string());
    let limit = std::env::var("LITHO_CODEX_QMD_LIMIT")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(8)
        .clamp(1, 32);
    let index = std::env::var("LITHO_CODEX_QMD_INDEX")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let query = qmd_query_for_section(codebase, section);

    let mut cmd = Command::new(bin);
    cmd.arg(mode)
        .arg(query)
        .arg("--json")
        .arg("--limit")
        .arg(limit.to_string())
        .current_dir(project_path);
    if let Some(index) = index {
        cmd.arg("--index").arg(index);
    }

    let output = match cmd.output() {
        Ok(out) if out.status.success() => out,
        _ => return Vec::new(),
    };

    match serde_json::from_slice::<RetrievedSearchResponse>(&output.stdout) {
        Ok(parsed) => dedup_snippets(parsed.results),
        Err(_) => Vec::new(),
    }
}

fn qmd_query_for_section(codebase: &ExtractedCodebase, section: &str) -> String {
    match section {
        "architecture" => format!(
            "{} architecture components boundaries interfaces design decisions",
            codebase.project_name
        ),
        "workflows" => format!(
            "{} workflows request flow lifecycle orchestration sequence",
            codebase.project_name
        ),
        "database" => format!(
            "{} database schema storage models persistence indexing",
            codebase.project_name
        ),
        _ => format!(
            "{} architecture workflows boundaries key modules interfaces",
            codebase.project_name
        ),
    }
}

fn dedup_snippets(snippets: Vec<RetrievedSnippet>) -> Vec<RetrievedSnippet> {
    let mut by_file = BTreeMap::<String, RetrievedSnippet>::new();
    for snippet in snippets {
        let key = snippet.file.clone();
        match by_file.get(&key) {
            Some(existing) if existing.score >= snippet.score => {}
            _ => {
                by_file.insert(key, snippet);
            }
        }
    }
    by_file.into_values().collect()
}

fn is_truthy(value: Option<&str>) -> bool {
    matches!(
        value.unwrap_or("").trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}
