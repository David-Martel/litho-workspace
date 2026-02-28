use crate::generator::context::GeneratorContext;
use crate::types::original_document::OriginalDocument;
use anyhow::Result;
use std::path::Path;
use tokio::fs::read_to_string;

pub async fn extract(context: &GeneratorContext) -> Result<OriginalDocument> {
    let project_path = &context.config.project_path;

    // 1. Read README.md (primary project description)
    let readme = read_optional_file(&project_path.join("README.md")).await;
    let readme = readme.map(|content| trim_markdown(&content));

    // 2. Read supplementary docs (CLAUDE.md, CONTRIBUTING.md, docs/README.md)
    let supplementary_docs = read_supplementary_docs(project_path).await;

    // 3. Extract tech stack from manifest files
    let tech_stack = extract_tech_stack(project_path).await;

    Ok(OriginalDocument {
        readme,
        supplementary_docs,
        tech_stack,
    })
}

/// Read a file if it exists, returning None on any error.
async fn read_optional_file(path: &Path) -> Option<String> {
    read_to_string(path).await.ok()
}

/// Read supplementary documentation files that provide additional project context.
async fn read_supplementary_docs(project_path: &Path) -> Option<String> {
    let candidates = [
        "CLAUDE.md",
        "CONTRIBUTING.md",
        "docs/README.md",
    ];

    let mut combined = String::new();

    for filename in &candidates {
        let path = project_path.join(filename);
        if let Some(content) = read_optional_file(&path).await {
            let trimmed = trim_markdown(&content);
            if !trimmed.trim().is_empty() {
                combined.push_str(&format!("#### From {}\n", filename));
                // Limit each supplementary doc to prevent prompt bloat
                let truncated = if trimmed.len() > 4000 {
                    format!("{}...(truncated)", &trimmed[..4000])
                } else {
                    trimmed
                };
                combined.push_str(&truncated);
                combined.push_str("\n\n");
            }
        }
    }

    if combined.is_empty() {
        None
    } else {
        Some(combined)
    }
}

/// Extract dependency/technology names from common manifest files.
///
/// Returns a flat list like ["rust: tokio, serde, clap", "python: pydantic, httpx"].
async fn extract_tech_stack(project_path: &Path) -> Option<Vec<String>> {
    let mut stack = Vec::new();

    // Cargo.toml → Rust dependencies
    if let Some(content) = read_optional_file(&project_path.join("Cargo.toml")).await {
        let deps = parse_cargo_deps(&content);
        if !deps.is_empty() {
            stack.push(format!("Rust: {}", deps.join(", ")));
        }
    }

    // pyproject.toml → Python dependencies
    if let Some(content) = read_optional_file(&project_path.join("pyproject.toml")).await {
        let deps = parse_pyproject_deps(&content);
        if !deps.is_empty() {
            stack.push(format!("Python: {}", deps.join(", ")));
        }
    }

    // package.json → Node.js dependencies
    if let Some(content) = read_optional_file(&project_path.join("package.json")).await {
        let deps = parse_package_json_deps(&content);
        if !deps.is_empty() {
            stack.push(format!("Node.js: {}", deps.join(", ")));
        }
    }

    // requirements.txt → Python dependencies (fallback)
    if let Some(content) = read_optional_file(&project_path.join("requirements.txt")).await {
        let deps = parse_requirements_txt(&content);
        if !deps.is_empty() && !stack.iter().any(|s| s.starts_with("Python:")) {
            stack.push(format!("Python: {}", deps.join(", ")));
        }
    }

    if stack.is_empty() {
        None
    } else {
        Some(stack)
    }
}

/// Trim markdown content while preserving structural headings.
///
/// Unlike the previous version that stripped all headings, this preserves `#` lines
/// because headings like "## Architecture" provide crucial structural context for the LLM.
fn trim_markdown(markdown: &str) -> String {
    let mut description = String::new();
    let mut in_code_block = false;

    for line in markdown.lines().take(500) {
        // Track code fence state
        if line.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        // Skip lines inside code blocks
        if in_code_block {
            continue;
        }
        if !line.trim().is_empty() {
            description.push_str(line);
            description.push('\n');
        }
    }

    description
}

/// Parse dependency names from Cargo.toml [dependencies] section.
fn parse_cargo_deps(content: &str) -> Vec<String> {
    let mut deps = Vec::new();
    let mut in_deps = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[dependencies]" || trimmed == "[dev-dependencies]" || trimmed == "[build-dependencies]" {
            in_deps = trimmed == "[dependencies]";
            continue;
        }
        if trimmed.starts_with('[') {
            in_deps = false;
            continue;
        }
        if in_deps {
            // Lines like: tokio = "1" or tokio = { version = "1", features = [...] }
            if let Some(name) = trimmed.split('=').next() {
                let name = name.trim();
                if !name.is_empty() && !name.starts_with('#') {
                    deps.push(name.to_string());
                }
            }
        }
    }

    deps.truncate(30); // Cap at 30 to avoid prompt bloat
    deps
}

/// Parse dependency names from pyproject.toml.
fn parse_pyproject_deps(content: &str) -> Vec<String> {
    let mut deps = Vec::new();
    let mut in_deps = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "dependencies = [" || trimmed.starts_with("dependencies = [") {
            in_deps = true;
            // Handle inline array: dependencies = ["foo", "bar"]
            if let Some(rest) = trimmed.strip_prefix("dependencies = [") {
                for item in rest.trim_end_matches(']').split(',') {
                    if let Some(name) = extract_pyproject_dep_name(item.trim()) {
                        deps.push(name);
                    }
                }
                if rest.contains(']') {
                    in_deps = false;
                }
            }
            continue;
        }
        if in_deps {
            if trimmed == "]" || trimmed.starts_with(']') {
                in_deps = false;
                continue;
            }
            if let Some(name) = extract_pyproject_dep_name(trimmed) {
                deps.push(name);
            }
        }
    }

    deps.truncate(30);
    deps
}

/// Extract package name from a pyproject dependency string like `"httpx>=0.27"`.
fn extract_pyproject_dep_name(s: &str) -> Option<String> {
    let s = s.trim().trim_matches(',').trim_matches('"').trim_matches('\'');
    if s.is_empty() {
        return None;
    }
    // Split on version specifiers: >=, <=, ==, !=, ~=, <, >
    let name = s
        .split(&['>', '<', '=', '!', '~', '[', ';'][..])
        .next()
        .unwrap_or(s)
        .trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// Parse dependency names from package.json.
fn parse_package_json_deps(content: &str) -> Vec<String> {
    let mut deps = Vec::new();

    if let Ok(json) = serde_json::from_str::<serde_json::Value>(content) {
        if let Some(obj) = json.get("dependencies").and_then(|v| v.as_object()) {
            for key in obj.keys() {
                deps.push(key.clone());
            }
        }
    }

    deps.truncate(30);
    deps
}

/// Parse dependency names from requirements.txt.
fn parse_requirements_txt(content: &str) -> Vec<String> {
    let mut deps = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('-') {
            continue;
        }
        // Split on version specifiers
        let name = trimmed
            .split(&['>', '<', '=', '!', '~', '[', ';'][..])
            .next()
            .unwrap_or(trimmed)
            .trim();
        if !name.is_empty() {
            deps.push(name.to_string());
        }
    }

    deps.truncate(30);
    deps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trim_markdown_preserves_headings() {
        let input = "# Title\n## Section\nSome content\n```\ncode block\n```\nMore content";
        let result = trim_markdown(input);
        assert!(result.contains("# Title"));
        assert!(result.contains("## Section"));
        assert!(result.contains("Some content"));
        assert!(result.contains("More content"));
        assert!(!result.contains("code block"));
    }

    #[test]
    fn test_parse_cargo_deps() {
        let input = r#"
[package]
name = "test"

[dependencies]
tokio = "1"
serde = { version = "1", features = ["derive"] }

[dev-dependencies]
tempfile = "3"
"#;
        let deps = parse_cargo_deps(input);
        assert_eq!(deps, vec!["tokio", "serde"]);
    }

    #[test]
    fn test_parse_requirements_txt() {
        let input = "httpx>=0.27\nfastapi\n# comment\npydantic>=2.0";
        let deps = parse_requirements_txt(input);
        assert_eq!(deps, vec!["httpx", "fastapi", "pydantic"]);
    }

    #[test]
    fn test_extract_pyproject_dep_name() {
        assert_eq!(extract_pyproject_dep_name("\"httpx>=0.27\""), Some("httpx".into()));
        assert_eq!(extract_pyproject_dep_name("\"pydantic\""), Some("pydantic".into()));
        assert_eq!(extract_pyproject_dep_name(""), None);
    }

    // --- New tests ---

    #[test]
    fn test_trim_markdown_skips_code_fence_content() {
        let input = "Before\n```rust\nlet x = 1;\nlet y = 2;\n```\nAfter";
        let result = trim_markdown(input);
        assert!(result.contains("Before"));
        assert!(result.contains("After"));
        assert!(!result.contains("let x = 1;"));
        assert!(!result.contains("let y = 2;"));
        // The fence delimiters themselves are also excluded
        assert!(!result.contains("```"));
    }

    #[test]
    fn test_trim_markdown_skips_blank_lines() {
        let input = "Line one\n\n\nLine two\n\nLine three";
        let result = trim_markdown(input);
        // Content lines preserved, blank lines collapsed
        assert!(result.contains("Line one"));
        assert!(result.contains("Line two"));
        assert!(result.contains("Line three"));
        // No double blank lines in output (each blank line is stripped)
        assert!(!result.contains("\n\n\n"));
    }

    #[test]
    fn test_trim_markdown_500_line_truncation() {
        // Build a document with 600 non-empty lines
        let input: String = (1..=600).map(|i| format!("Line {}\n", i)).collect();
        let result = trim_markdown(&input);
        let line_count = result.lines().count();
        // Must not exceed 500 content lines
        assert!(
            line_count <= 500,
            "Expected at most 500 lines, got {}",
            line_count
        );
        // The 500th line must appear, the 501st must not
        assert!(result.contains("Line 500"));
        assert!(!result.contains("Line 501"));
    }

    #[test]
    fn test_trim_markdown_empty_input() {
        assert_eq!(trim_markdown(""), "");
    }

    #[test]
    fn test_trim_markdown_preserves_all_heading_levels() {
        let input = "# H1\n## H2\n### H3\n#### H4\n##### H5\n###### H6\ntext";
        let result = trim_markdown(input);
        for heading in &["# H1", "## H2", "### H3", "#### H4", "##### H5", "###### H6"] {
            assert!(result.contains(heading), "Missing heading: {}", heading);
        }
    }

    #[test]
    fn test_parse_cargo_deps_excludes_dev_and_build_deps() {
        let input = r#"
[dependencies]
tokio = "1"

[dev-dependencies]
criterion = "0.5"

[build-dependencies]
cc = "1"
"#;
        let deps = parse_cargo_deps(input);
        // Only [dependencies] entries should be returned
        assert!(deps.contains(&"tokio".to_string()));
        assert!(!deps.contains(&"criterion".to_string()));
        assert!(!deps.contains(&"cc".to_string()));
    }

    #[test]
    fn test_parse_cargo_deps_ignores_comments() {
        let input = r#"
[dependencies]
# this is a comment
tokio = "1"
# serde = "1"
"#;
        let deps = parse_cargo_deps(input);
        assert_eq!(deps, vec!["tokio"]);
    }

    #[test]
    fn test_parse_cargo_deps_caps_at_30() {
        // Generate more than 30 dependency lines
        let mut lines = String::from("[dependencies]\n");
        for i in 0..40 {
            lines.push_str(&format!("dep_{} = \"1\"\n", i));
        }
        let deps = parse_cargo_deps(&lines);
        assert_eq!(deps.len(), 30, "Should be capped at 30 entries");
    }

    #[test]
    fn test_parse_cargo_deps_empty_section() {
        let input = "[package]\nname = \"foo\"\n\n[dependencies]\n\n[features]\n";
        let deps = parse_cargo_deps(input);
        assert!(deps.is_empty());
    }

    #[test]
    fn test_parse_requirements_txt_skips_flags_and_urls() {
        let input = "-r other.txt\n-e git+https://github.com/foo/bar.git\nrequests==2.31\nnumpy";
        let deps = parse_requirements_txt(input);
        // Lines starting with '-' are skipped
        assert!(!deps.iter().any(|d| d.starts_with('-')));
        assert!(deps.contains(&"requests".to_string()));
        assert!(deps.contains(&"numpy".to_string()));
    }

    #[test]
    fn test_parse_requirements_txt_version_specifiers() {
        let input = "django>=4.0,<5.0\ncelery~=5.3\nredis!=4.0\nflask==2.3";
        let deps = parse_requirements_txt(input);
        assert!(deps.contains(&"django".to_string()));
        assert!(deps.contains(&"celery".to_string()));
        assert!(deps.contains(&"redis".to_string()));
        assert!(deps.contains(&"flask".to_string()));
        // No version specifiers should appear in the names
        for d in &deps {
            assert!(!d.contains('>'), "dep '{}' contains version specifier", d);
            assert!(!d.contains('='), "dep '{}' contains version specifier", d);
        }
    }

    #[test]
    fn test_parse_package_json_deps_basic() {
        let json = r#"{"dependencies": {"react": "^18.0.0", "axios": "^1.6.0"}}"#;
        let deps = parse_package_json_deps(json);
        assert!(deps.contains(&"react".to_string()));
        assert!(deps.contains(&"axios".to_string()));
    }

    #[test]
    fn test_parse_package_json_deps_missing_key() {
        // devDependencies only — no "dependencies" key
        let json = r#"{"devDependencies": {"jest": "^29.0.0"}}"#;
        let deps = parse_package_json_deps(json);
        assert!(deps.is_empty());
    }

    #[test]
    fn test_parse_package_json_deps_invalid_json() {
        let bad = "not json at all {{{";
        let deps = parse_package_json_deps(bad);
        assert!(deps.is_empty());
    }

    #[test]
    fn test_parse_package_json_deps_caps_at_30() {
        // Build a JSON object with 40 dependencies
        let entries: String = (0..40)
            .map(|i| format!("\"pkg_{}\": \"^1.0\"", i))
            .collect::<Vec<_>>()
            .join(",");
        let json = format!("{{\"dependencies\": {{{}}}}}", entries);
        let deps = parse_package_json_deps(&json);
        assert_eq!(deps.len(), 30, "Should be capped at 30 entries");
    }

    #[test]
    fn test_extract_pyproject_dep_name_with_extras() {
        // Package with extras like "requests[security]>=2.28"
        assert_eq!(
            extract_pyproject_dep_name("\"requests[security]>=2.28\""),
            Some("requests".into())
        );
    }

    #[test]
    fn test_extract_pyproject_dep_name_with_semicolon_marker() {
        // Conditional dependency: "pywin32; sys_platform == 'win32'"
        assert_eq!(
            extract_pyproject_dep_name("\"pywin32; sys_platform == 'win32'\""),
            Some("pywin32".into())
        );
    }

    #[test]
    fn test_parse_pyproject_deps_multiline_array() {
        let input = r#"
[project]
name = "myapp"
dependencies = [
    "httpx>=0.27",
    "pydantic>=2.0",
    "fastapi",
]
"#;
        let deps = parse_pyproject_deps(input);
        assert!(deps.contains(&"httpx".to_string()), "Expected httpx in {:?}", deps);
        assert!(deps.contains(&"pydantic".to_string()), "Expected pydantic in {:?}", deps);
        assert!(deps.contains(&"fastapi".to_string()), "Expected fastapi in {:?}", deps);
    }
}
