use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

use crate::generator::compose::memory::MemoryScope as ComposeMemory;
use crate::generator::compose::types::AgentType;
use crate::generator::context::GeneratorContext;
use crate::generator::preprocess::memory::{MemoryScope as PreprocessMemory, ScopedKeys};
use crate::types::code::CodeInsight;
use crate::types::project_structure::ProjectStructure;

/// Severity of a validation finding.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// A single validation finding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub severity: Severity,
    pub category: String,
    pub message: String,
    /// Which document section the finding applies to (e.g. "Architecture")
    pub section: Option<String>,
}

/// Aggregated validation report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationReport {
    pub findings: Vec<Finding>,
    /// Overall quality score (0.0 - 1.0)
    pub quality_score: f64,
    /// Per-category scores
    pub completeness_score: f64,
    pub accuracy_score: f64,
    pub freshness_score: f64,
    pub grounding_score: f64,
}

impl ValidationReport {
    pub fn errors(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Error)
            .count()
    }

    pub fn warnings(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Warning)
            .count()
    }
}

/// Content validator that cross-references generated documentation against
/// source truth from the preprocessing stage.
pub struct ContentValidator;

impl ContentValidator {
    /// Run all validation checks and produce a report.
    pub async fn validate(context: &GeneratorContext) -> Result<ValidationReport> {
        let mut findings = Vec::new();

        // Load preprocessing data from memory
        let project_structure: Option<ProjectStructure> = context
            .get_from_memory(PreprocessMemory::PREPROCESS, ScopedKeys::PROJECT_STRUCTURE)
            .await;
        let code_insights: Option<Vec<CodeInsight>> = context
            .get_from_memory(PreprocessMemory::PREPROCESS, ScopedKeys::CODE_INSIGHTS)
            .await;

        // Load generated documents from memory
        let doc_sections = Self::load_generated_docs(context).await;

        // 1. Structural completeness
        let completeness_score =
            Self::check_completeness(&doc_sections, &mut findings);

        // 2. File path accuracy
        let accuracy_score = if let Some(ref ps) = project_structure {
            Self::check_file_references(&doc_sections, ps, &mut findings)
        } else {
            findings.push(Finding {
                severity: Severity::Warning,
                category: "accuracy".to_string(),
                message: "No project structure in memory; skipping file reference checks"
                    .to_string(),
                section: None,
            });
            1.0
        };

        // 3. Freshness (stale references)
        let freshness_score = if let Some(ref ps) = project_structure {
            Self::check_freshness(&doc_sections, ps, &mut findings)
        } else {
            1.0
        };

        // 4. Tech stack grounding
        let grounding_score = Self::check_tech_grounding(
            &doc_sections,
            &context.config.project_path,
            code_insights.as_deref(),
            &mut findings,
        )
        .await;

        // Compute overall score (weighted average)
        let quality_score = completeness_score * 0.30
            + accuracy_score * 0.30
            + freshness_score * 0.15
            + grounding_score * 0.25;

        Ok(ValidationReport {
            findings,
            quality_score,
            completeness_score,
            accuracy_score,
            freshness_score,
            grounding_score,
        })
    }

    /// Load all generated document sections from memory.
    async fn load_generated_docs(
        context: &GeneratorContext,
    ) -> Vec<(String, String)> {
        let agent_types = [
            AgentType::Overview,
            AgentType::Architecture,
            AgentType::Workflow,
            AgentType::Boundary,
            AgentType::Database,
        ];

        let mut docs = Vec::new();
        for agent_type in &agent_types {
            let key = agent_type.to_string();
            if let Some(content) = context
                .get_from_memory::<String>(ComposeMemory::DOCUMENTATION, &key)
                .await
            {
                docs.push((key, content));
            }
        }
        docs
    }

    /// Check that all expected C4 document sections are present and non-trivial.
    fn check_completeness(
        doc_sections: &[(String, String)],
        findings: &mut Vec<Finding>,
    ) -> f64 {
        let expected = [
            "Project Overview",
            "Architecture Description",
            "Core Workflows",
            "Boundary Interfaces",
        ];

        let present_keys: HashSet<&str> = doc_sections
            .iter()
            .map(|(k, _)| k.as_str())
            .collect();

        let mut present_count = 0;
        let mut total = expected.len();

        for name in &expected {
            if present_keys.contains(*name) {
                let content = doc_sections
                    .iter()
                    .find(|(k, _)| k == name)
                    .map(|(_, c)| c.as_str())
                    .unwrap_or("");

                if content.len() < 100 {
                    findings.push(Finding {
                        severity: Severity::Error,
                        category: "completeness".to_string(),
                        message: format!("Section '{}' is present but too short ({} chars)", name, content.len()),
                        section: Some(name.to_string()),
                    });
                } else {
                    present_count += 1;
                }
            } else {
                findings.push(Finding {
                    severity: Severity::Error,
                    category: "completeness".to_string(),
                    message: format!("Required section '{}' is missing", name),
                    section: Some(name.to_string()),
                });
            }
        }

        // Database Overview is optional (not all projects have databases)
        if present_keys.contains("Database Overview") {
            let content = doc_sections
                .iter()
                .find(|(k, _)| k == "Database Overview")
                .map(|(_, c)| c.as_str())
                .unwrap_or("");
            if content.len() >= 100 {
                present_count += 1;
            }
            total += 1;
        }

        present_count as f64 / total as f64
    }

    /// Check that file paths mentioned in docs exist on disk.
    fn check_file_references(
        doc_sections: &[(String, String)],
        project_structure: &ProjectStructure,
        findings: &mut Vec<Finding>,
    ) -> f64 {
        // Build a set of known file paths from project structure
        let known_paths: HashSet<String> = project_structure
            .files
            .iter()
            .map(|f| f.path.to_string_lossy().to_string())
            .collect();

        // Also collect just filenames for fuzzy matching
        let known_filenames: HashSet<String> = project_structure
            .files
            .iter()
            .filter_map(|f| {
                f.path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
            })
            .collect();

        // Extract file references from markdown (backtick-wrapped paths)
        let path_re = Regex::new(r"`([^`]+\.[a-zA-Z]{1,10})`").unwrap();

        let mut total_refs = 0usize;
        let mut valid_refs = 0usize;

        for (section, content) in doc_sections {
            for cap in path_re.captures_iter(content) {
                let referenced = &cap[1];

                // Skip URLs, version strings, and config values
                if referenced.contains("://")
                    || referenced.starts_with("v")
                    || referenced.contains('=')
                {
                    continue;
                }

                // Skip common non-file references
                if referenced.ends_with(".com")
                    || referenced.ends_with(".org")
                    || referenced.ends_with(".io")
                {
                    continue;
                }

                total_refs += 1;

                // Check exact path match or filename match
                let filename = Path::new(referenced)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();

                if known_paths.contains(referenced) || known_filenames.contains(&filename) {
                    valid_refs += 1;
                } else {
                    findings.push(Finding {
                        severity: Severity::Warning,
                        category: "accuracy".to_string(),
                        message: format!(
                            "Referenced file '{}' not found in project structure",
                            referenced
                        ),
                        section: Some(section.clone()),
                    });
                }
            }
        }

        if total_refs == 0 {
            // No file references found — not necessarily bad
            findings.push(Finding {
                severity: Severity::Info,
                category: "accuracy".to_string(),
                message: "No file path references found in generated documentation".to_string(),
                section: None,
            });
            return 1.0;
        }

        valid_refs as f64 / total_refs as f64
    }

    /// Check for references to potentially renamed or deleted files.
    fn check_freshness(
        doc_sections: &[(String, String)],
        project_structure: &ProjectStructure,
        findings: &mut Vec<Finding>,
    ) -> f64 {
        let root = &project_structure.root_path;

        // Extract all backtick-enclosed paths that look like file references
        let path_re = Regex::new(r"`([a-zA-Z0-9_/\\.-]+\.[a-zA-Z]{1,10})`").unwrap();

        let mut total_checked = 0usize;
        let mut fresh_count = 0usize;

        for (section, content) in doc_sections {
            for cap in path_re.captures_iter(content) {
                let referenced = &cap[1];

                // Only check paths that look like relative project paths
                if referenced.contains("://") || referenced.starts_with("v") {
                    continue;
                }
                if referenced.ends_with(".com")
                    || referenced.ends_with(".org")
                    || referenced.ends_with(".io")
                {
                    continue;
                }

                // If it looks like a relative path, check if it exists on disk
                let candidate = root.join(referenced);
                if candidate.exists() {
                    fresh_count += 1;
                    total_checked += 1;
                } else {
                    // Not necessarily stale — might be a partial path or code reference
                    // Only flag paths with directory separators (more likely actual paths)
                    if referenced.contains('/') || referenced.contains('\\') {
                        total_checked += 1;
                        findings.push(Finding {
                            severity: Severity::Warning,
                            category: "freshness".to_string(),
                            message: format!(
                                "Path '{}' referenced but does not exist on disk",
                                referenced
                            ),
                            section: Some(section.clone()),
                        });
                    }
                }
            }
        }

        if total_checked == 0 {
            return 1.0;
        }

        fresh_count as f64 / total_checked as f64
    }

    /// Check that tech stack mentions are grounded in actual manifest files.
    async fn check_tech_grounding(
        doc_sections: &[(String, String)],
        project_path: &Path,
        code_insights: Option<&[CodeInsight]>,
        findings: &mut Vec<Finding>,
    ) -> f64 {
        // Collect known technologies from manifest files
        let mut known_tech: HashSet<String> = HashSet::new();

        // Parse Cargo.toml dependencies
        let cargo_toml = project_path.join("Cargo.toml");
        if cargo_toml.exists() {
            if let Ok(content) = tokio::fs::read_to_string(&cargo_toml).await {
                known_tech.insert("rust".to_string());
                known_tech.insert("cargo".to_string());
                Self::extract_cargo_deps(&content, &mut known_tech);
            }
        }

        // Parse package.json dependencies
        let package_json = project_path.join("package.json");
        if package_json.exists() {
            if let Ok(content) = tokio::fs::read_to_string(&package_json).await {
                known_tech.insert("node".to_string());
                known_tech.insert("npm".to_string());
                Self::extract_npm_deps(&content, &mut known_tech);
            }
        }

        // Parse pyproject.toml / requirements.txt
        let pyproject = project_path.join("pyproject.toml");
        if pyproject.exists() {
            known_tech.insert("python".to_string());
        }
        let requirements = project_path.join("requirements.txt");
        if requirements.exists() {
            known_tech.insert("python".to_string());
            if let Ok(content) = tokio::fs::read_to_string(&requirements).await {
                Self::extract_pip_deps(&content, &mut known_tech);
            }
        }

        // Add languages detected from code insights
        if let Some(insights) = code_insights {
            for insight in insights {
                let ext = insight
                    .code_dossier
                    .file_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");
                match ext {
                    "rs" => {
                        known_tech.insert("rust".to_string());
                    }
                    "ts" | "tsx" => {
                        known_tech.insert("typescript".to_string());
                    }
                    "js" | "jsx" => {
                        known_tech.insert("javascript".to_string());
                    }
                    "py" => {
                        known_tech.insert("python".to_string());
                    }
                    "cs" => {
                        known_tech.insert("csharp".to_string());
                        known_tech.insert("c#".to_string());
                    }
                    _ => {}
                }
            }
        }

        if known_tech.is_empty() {
            findings.push(Finding {
                severity: Severity::Info,
                category: "grounding".to_string(),
                message: "No manifest files found; skipping tech grounding checks".to_string(),
                section: None,
            });
            return 1.0;
        }

        // Look for technology/framework claims in docs
        let tech_claim_re =
            Regex::new(r"(?i)\b(built with|uses|powered by|written in|framework|stack)\b")
                .unwrap();

        let mut grounded_claims = 0usize;
        let mut total_claims = 0usize;

        for (section, content) in doc_sections {
            for line in content.lines() {
                if tech_claim_re.is_match(line) {
                    total_claims += 1;
                    // Check if the line mentions any known technology
                    let line_lower = line.to_lowercase();
                    if known_tech.iter().any(|tech| line_lower.contains(tech)) {
                        grounded_claims += 1;
                    } else {
                        findings.push(Finding {
                            severity: Severity::Warning,
                            category: "grounding".to_string(),
                            message: format!(
                                "Tech claim not verified against manifest: {}",
                                line.trim().chars().take(120).collect::<String>()
                            ),
                            section: Some(section.clone()),
                        });
                    }
                }
            }
        }

        if total_claims == 0 {
            return 1.0;
        }

        grounded_claims as f64 / total_claims as f64
    }

    /// Extract dependency names from Cargo.toml content.
    fn extract_cargo_deps(content: &str, known_tech: &mut HashSet<String>) {
        // Simple line-by-line parsing for [dependencies] entries
        let dep_re = Regex::new(r#"^(\w[\w-]*)\s*="#).unwrap();
        let mut in_deps = false;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("[dependencies")
                || trimmed.starts_with("[dev-dependencies")
                || trimmed.starts_with("[build-dependencies")
            {
                in_deps = true;
                continue;
            }
            if trimmed.starts_with('[') {
                in_deps = false;
                continue;
            }
            if in_deps {
                if let Some(cap) = dep_re.captures(trimmed) {
                    let dep_name = cap[1].to_lowercase().replace('-', "_");
                    known_tech.insert(dep_name);
                    // Also insert the original crate name
                    known_tech.insert(cap[1].to_lowercase());
                }
            }
        }
    }

    /// Extract dependency names from package.json content.
    fn extract_npm_deps(content: &str, known_tech: &mut HashSet<String>) {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(content) {
            for key in ["dependencies", "devDependencies"] {
                if let Some(deps) = val.get(key).and_then(|d| d.as_object()) {
                    for dep_name in deps.keys() {
                        // Strip @scope/ prefix
                        let name = dep_name
                            .strip_prefix('@')
                            .and_then(|s| s.split('/').nth(1))
                            .unwrap_or(dep_name);
                        known_tech.insert(name.to_lowercase());
                    }
                }
            }
        }
    }

    /// Extract dependency names from requirements.txt content.
    fn extract_pip_deps(content: &str, known_tech: &mut HashSet<String>) {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            // "package==1.0" or "package>=1.0" or just "package"
            let name = trimmed
                .split(&['=', '>', '<', '!', ';', '['][..])
                .next()
                .unwrap_or("")
                .trim()
                .to_lowercase();
            if !name.is_empty() {
                known_tech.insert(name);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_cargo_deps() {
        let content = r#"
[package]
name = "litho-generator"

[dependencies]
tokio = { version = "1.0" }
serde = "1.0"
anyhow = "1"

[dev-dependencies]
tempfile = "3"
"#;
        let mut tech = HashSet::new();
        ContentValidator::extract_cargo_deps(content, &mut tech);
        assert!(tech.contains("tokio"));
        assert!(tech.contains("serde"));
        assert!(tech.contains("anyhow"));
        assert!(tech.contains("tempfile"));
    }

    #[test]
    fn test_extract_npm_deps() {
        let content = r#"
{
    "dependencies": {
        "react": "^18.0",
        "@types/node": "^20.0"
    },
    "devDependencies": {
        "typescript": "^5.0"
    }
}
"#;
        let mut tech = HashSet::new();
        ContentValidator::extract_npm_deps(content, &mut tech);
        assert!(tech.contains("react"));
        assert!(tech.contains("node"));
        assert!(tech.contains("typescript"));
    }

    #[test]
    fn test_extract_pip_deps() {
        let content = "# requirements\nflask>=2.0\nrequests==2.28\nnumpy\n";
        let mut tech = HashSet::new();
        ContentValidator::extract_pip_deps(content, &mut tech);
        assert!(tech.contains("flask"));
        assert!(tech.contains("requests"));
        assert!(tech.contains("numpy"));
    }

    #[test]
    fn test_completeness_all_present() {
        let docs = vec![
            ("Project Overview".to_string(), "x".repeat(200)),
            ("Architecture Description".to_string(), "x".repeat(200)),
            ("Core Workflows".to_string(), "x".repeat(200)),
            ("Boundary Interfaces".to_string(), "x".repeat(200)),
        ];
        let mut findings = Vec::new();
        let score = ContentValidator::check_completeness(&docs, &mut findings);
        assert!((score - 1.0).abs() < f64::EPSILON);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_completeness_missing_section() {
        let docs = vec![
            ("Project Overview".to_string(), "x".repeat(200)),
            ("Architecture Description".to_string(), "x".repeat(200)),
        ];
        let mut findings = Vec::new();
        let score = ContentValidator::check_completeness(&docs, &mut findings);
        assert!(score < 1.0);
        assert!(findings.iter().any(|f| f.severity == Severity::Error));
    }

    #[test]
    fn test_completeness_short_section() {
        let docs = vec![
            ("Project Overview".to_string(), "short".to_string()),
            ("Architecture Description".to_string(), "x".repeat(200)),
            ("Core Workflows".to_string(), "x".repeat(200)),
            ("Boundary Interfaces".to_string(), "x".repeat(200)),
        ];
        let mut findings = Vec::new();
        let score = ContentValidator::check_completeness(&docs, &mut findings);
        assert!(score < 1.0);
        assert!(findings
            .iter()
            .any(|f| f.message.contains("too short")));
    }

    #[test]
    fn test_validation_report_counts() {
        let report = ValidationReport {
            findings: vec![
                Finding {
                    severity: Severity::Error,
                    category: "test".into(),
                    message: "err".into(),
                    section: None,
                },
                Finding {
                    severity: Severity::Warning,
                    category: "test".into(),
                    message: "warn1".into(),
                    section: None,
                },
                Finding {
                    severity: Severity::Warning,
                    category: "test".into(),
                    message: "warn2".into(),
                    section: None,
                },
                Finding {
                    severity: Severity::Info,
                    category: "test".into(),
                    message: "info".into(),
                    section: None,
                },
            ],
            quality_score: 0.75,
            completeness_score: 1.0,
            accuracy_score: 0.5,
            freshness_score: 1.0,
            grounding_score: 0.5,
        };
        assert_eq!(report.errors(), 1);
        assert_eq!(report.warnings(), 2);
    }

    #[test]
    fn test_cargo_deps_with_workspace_section() {
        let content = r#"
[workspace]
members = ["crates/*"]

[workspace.dependencies]
tokio = "1"
serde = "1"

[dependencies]
reqwest = { version = "0.12" }
"#;
        let mut tech = HashSet::new();
        ContentValidator::extract_cargo_deps(content, &mut tech);
        // workspace.dependencies isn't parsed by this simple parser — that's OK
        assert!(tech.contains("reqwest"));
    }

    #[test]
    fn test_file_references_no_paths() {
        let docs = vec![(
            "Overview".to_string(),
            "This project is great with no file references.".to_string(),
        )];
        let ps = ProjectStructure {
            project_name: "test".into(),
            root_path: "/tmp".into(),
            directories: vec![],
            files: vec![],
            total_files: 0,
            total_directories: 0,
            file_types: Default::default(),
            size_distribution: Default::default(),
        };
        let mut findings = Vec::new();
        let score = ContentValidator::check_file_references(&docs, &ps, &mut findings);
        assert!((score - 1.0).abs() < f64::EPSILON);
    }
}
