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
    let s = s.trim().trim_matches('"').trim_matches('\'').trim_matches(',');
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
}
