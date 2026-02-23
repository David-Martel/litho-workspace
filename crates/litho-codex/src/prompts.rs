use litho_core::types::ExtractedCodebase;

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
