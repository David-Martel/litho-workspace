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
    pub coherence_score: f64,
    /// LLM-as-judge helpfulness score (0.0 - 1.0).
    /// Defaults to 1.0 when Ollama is unavailable.
    pub helpfulness_score: f64,
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

        // 1. Structural completeness (section coverage + symbol coverage)
        let section_completeness = Self::check_completeness(&doc_sections, &mut findings);
        let symbol_coverage =
            Self::check_symbol_coverage(&doc_sections, code_insights.as_deref(), &mut findings);
        // Blend: 60% section structure, 40% symbol coverage
        let completeness_score = section_completeness * 0.6 + symbol_coverage * 0.4;

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

        // 5. Terminology coherence
        let coherence_score = Self::check_coherence(&doc_sections, &mut findings);

        // 6. Helpfulness (LLM-as-judge, optional — returns 1.0 when Ollama unavailable)
        let helpfulness_score =
            Self::check_helpfulness(&doc_sections, &context.config, &mut findings).await;

        // Compute overall score (weighted average from config)
        let qc = &context.config.quality;
        let quality_score = completeness_score * qc.completeness_weight
            + accuracy_score * qc.accuracy_weight
            + freshness_score * qc.freshness_weight
            + grounding_score * qc.grounding_weight
            + coherence_score * qc.coherence_weight
            + helpfulness_score * qc.helpfulness_weight;

        Ok(ValidationReport {
            findings,
            quality_score,
            completeness_score,
            accuracy_score,
            freshness_score,
            grounding_score,
            coherence_score,
            helpfulness_score,
        })
    }

    /// Load all generated document sections from memory.
    async fn load_generated_docs(context: &GeneratorContext) -> Vec<(String, String)> {
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
    pub fn check_completeness(
        doc_sections: &[(String, String)],
        findings: &mut Vec<Finding>,
    ) -> f64 {
        let expected = [
            "Project Overview",
            "Architecture Description",
            "Core Workflows",
            "Boundary Interfaces",
        ];

        let present_keys: HashSet<&str> = doc_sections.iter().map(|(k, _)| k.as_str()).collect();

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
                        message: format!(
                            "Section '{}' is present but too short ({} chars)",
                            name,
                            content.len()
                        ),
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
            .filter_map(|f| f.path.file_name().map(|n| n.to_string_lossy().to_string()))
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
        if cargo_toml.exists()
            && let Ok(content) = tokio::fs::read_to_string(&cargo_toml).await
        {
            known_tech.insert("rust".to_string());
            known_tech.insert("cargo".to_string());
            Self::extract_cargo_deps(&content, &mut known_tech);
        }

        // Parse package.json dependencies
        let package_json = project_path.join("package.json");
        if package_json.exists()
            && let Ok(content) = tokio::fs::read_to_string(&package_json).await
        {
            known_tech.insert("node".to_string());
            known_tech.insert("npm".to_string());
            Self::extract_npm_deps(&content, &mut known_tech);
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
            Regex::new(r"(?i)\b(built with|uses|powered by|written in|framework|stack)\b").unwrap();

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

    /// Check symbol coverage: what fraction of public symbols appear in docs.
    ///
    /// Uses `CodeInsight` data (from preprocessing) as ground truth.
    /// A symbol is "documented" if its name appears anywhere in the generated docs.
    ///
    /// Returns `1.0` when no insights are available (nothing to check).
    pub fn check_symbol_coverage(
        doc_sections: &[(String, String)],
        code_insights: Option<&[CodeInsight]>,
        findings: &mut Vec<Finding>,
    ) -> f64 {
        let insights = match code_insights {
            Some(i) if !i.is_empty() => i,
            _ => return 1.0, // No insights — can't check
        };

        // Collect all public symbol names from code insights.
        let mut public_symbols: Vec<String> = Vec::new();
        for insight in insights {
            // Functions from code dossier
            for func in &insight.code_dossier.functions {
                if !func.is_empty() {
                    public_symbols.push(func.clone());
                }
            }
            // Interface names from code dossier
            for iface in &insight.code_dossier.interfaces {
                if !iface.is_empty() {
                    public_symbols.push(iface.clone());
                }
            }
            // Detailed interface names
            for iface in &insight.interfaces {
                if !iface.name.is_empty() {
                    public_symbols.push(iface.name.clone());
                }
            }
        }

        // Deduplicate
        public_symbols.sort();
        public_symbols.dedup();

        if public_symbols.is_empty() {
            return 1.0;
        }

        // Concatenate all doc content for searching.
        let all_docs: String = doc_sections
            .iter()
            .map(|(_, content)| content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        let mut documented = 0usize;
        let mut undocumented: Vec<String> = Vec::new();

        for symbol in &public_symbols {
            let found = if symbol.len() <= 4 {
                // Short symbols need word-boundary matching to avoid false positives
                // (e.g., "id" matching "considered", "to" matching "into").
                Regex::new(&format!(r"\b{}\b", regex::escape(symbol)))
                    .map(|re| re.is_match(&all_docs))
                    .unwrap_or(false)
            } else {
                all_docs.contains(symbol.as_str())
            };

            if found {
                documented += 1;
            } else {
                undocumented.push(symbol.clone());
            }
        }

        // Report undocumented symbols (up to 20 in a single finding).
        if !undocumented.is_empty() {
            let sample: Vec<&str> = undocumented.iter().take(20).map(|s| s.as_str()).collect();
            findings.push(Finding {
                severity: Severity::Warning,
                category: "completeness".to_string(),
                message: format!(
                    "{}/{} public symbols not found in docs: {}{}",
                    undocumented.len(),
                    public_symbols.len(),
                    sample.join(", "),
                    if undocumented.len() > 20 { "..." } else { "" }
                ),
                section: None,
            });
        }

        documented as f64 / public_symbols.len() as f64
    }

    /// Check terminology consistency across all doc sections.
    ///
    /// Extracts backtick-wrapped terms from each section and flags inconsistencies
    /// where similar names appear with different spellings (e.g. `CodeInsight` vs
    /// `code_insight` vs `Code Insight`).
    ///
    /// Returns a score in `[0.0, 1.0]` where `1.0` means fully consistent.
    pub fn check_coherence(doc_sections: &[(String, String)], findings: &mut Vec<Finding>) -> f64 {
        use std::collections::HashMap;

        // Extract all backtick-wrapped identifiers (3+ chars, starts with a letter/underscore).
        let term_re = Regex::new(r"`([A-Za-z_]\w{2,})`").unwrap();
        let mut term_counts: HashMap<String, usize> = HashMap::new();

        for (_, content) in doc_sections {
            for cap in term_re.captures_iter(content) {
                let term = cap[1].to_string();
                *term_counts.entry(term).or_insert(0) += 1;
            }
        }

        if term_counts.is_empty() {
            return 1.0;
        }

        // Group terms by their normalized form (lowercase, strip underscores/hyphens).
        let mut normalized_groups: HashMap<String, Vec<String>> = HashMap::new();
        for term in term_counts.keys() {
            let normalized = term.to_lowercase().replace(['_', '-'], "");
            normalized_groups
                .entry(normalized)
                .or_default()
                .push(term.clone());
        }

        // Filter out groups where all variants are well-known Rust casing conventions
        // for conversion/accessor trait method pairs (e.g., `to_string` vs `ToString`,
        // `as_ref` vs `AsRef`, `into_iter` vs `IntoIter`).
        // These are expected naming patterns in Rust and are not actual inconsistencies.
        // Only filter when the snake_case form starts with a Rust conversion method prefix.
        let rust_conversion_prefix = Regex::new(r"^(to|as|into|from|try_from|try_into)_").unwrap();
        normalized_groups.retain(|_, variants| {
            if variants.len() != 2 {
                return true; // Only handle the simple 2-variant case
            }
            // Check if exactly one is snake_case starting with a conversion prefix,
            // and exactly one is PascalCase — these are Rust trait method naming conventions.
            let mut snake_conversion: Option<&str> = None;
            let mut has_pascal = false;
            for v in variants.iter() {
                if v.contains('_') && rust_conversion_prefix.is_match(v) {
                    snake_conversion = Some(v.as_str());
                } else if v.chars().next().is_some_and(|c| c.is_uppercase()) {
                    has_pascal = true;
                }
            }
            // If one variant is a Rust conversion method (to_*, as_*, into_*, from_*)
            // and the other is PascalCase, treat this as an expected naming convention.
            if snake_conversion.is_some() && has_pascal {
                return false; // Exclude this group
            }
            true
        });

        // Find groups that have more than one spelling variant.
        let mut inconsistencies = 0usize;
        for variants in normalized_groups.values() {
            if variants.len() > 1 {
                inconsistencies += 1;
                // Only emit individual findings for the first 10 groups.
                if inconsistencies <= 10 {
                    let mut sorted = variants.clone();
                    sorted.sort();
                    findings.push(Finding {
                        severity: Severity::Warning,
                        category: "coherence".to_string(),
                        message: format!("Inconsistent terminology: {}", sorted.join(" vs ")),
                        section: None,
                    });
                }
            }
        }

        if inconsistencies > 10 {
            findings.push(Finding {
                severity: Severity::Warning,
                category: "coherence".to_string(),
                message: format!(
                    "... and {} more terminology inconsistencies",
                    inconsistencies - 10
                ),
                section: None,
            });
        }

        // Score: penalize by the ratio of inconsistent groups to total groups.
        let total_groups = normalized_groups.len();
        if total_groups == 0 {
            return 1.0;
        }

        let consistent_groups = total_groups - inconsistencies;
        consistent_groups as f64 / total_groups as f64
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
            if in_deps && let Some(cap) = dep_re.captures(trimmed) {
                let dep_name = cap[1].to_lowercase().replace('-', "_");
                known_tech.insert(dep_name);
                // Also insert the original crate name
                known_tech.insert(cap[1].to_lowercase());
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

    /// Rate documentation helpfulness using the local LLM as a judge (G-Eval style).
    ///
    /// Sends each documentation section to the configured Ollama model and asks
    /// it to rate four dimensions on a 1–5 scale:
    ///
    /// * `summary_quality` — how accurately the opening captures the section's purpose
    /// * `depth` — whether the section explains *why*, not just *what*
    /// * `actionability` — usefulness to a new contributor
    /// * `examples` — presence of code references, file paths, or usage examples
    ///
    /// The four sub-scores are averaged per section and then across all sections,
    /// then normalized to `[0.0, 1.0]`.
    ///
    /// Returns `1.0` (neutral) when:
    /// * The configured provider is not `Ollama`
    /// * The Ollama client cannot be constructed
    /// * No sections meet the minimum length threshold (100 chars)
    async fn check_helpfulness(
        doc_sections: &[(String, String)],
        config: &crate::config::Config,
        findings: &mut Vec<Finding>,
    ) -> f64 {
        use crate::llm::client::ollama_native::OllamaNativeClient;

        // Only run when Ollama provider is configured.
        if config.llm.provider != crate::config::LLMProvider::Ollama {
            findings.push(Finding {
                severity: Severity::Info,
                category: "helpfulness".to_string(),
                message: "LLM-as-judge requires Ollama provider; skipping helpfulness check"
                    .to_string(),
                section: None,
            });
            return 1.0;
        }

        let client = match OllamaNativeClient::from_config(&config.llm) {
            Ok(c) => c,
            Err(e) => {
                findings.push(Finding {
                    severity: Severity::Warning,
                    category: "helpfulness".to_string(),
                    message: format!("Could not connect to Ollama for helpfulness check: {}", e),
                    section: None,
                });
                return 1.0;
            }
        };

        if config.llm.model_efficient.is_empty() {
            findings.push(Finding {
                severity: Severity::Warning,
                category: "helpfulness".to_string(),
                message: "model_efficient is empty; skipping helpfulness check".to_string(),
                section: None,
            });
            return 1.0;
        }

        let system_prompt = r#"You are a documentation quality evaluator. Rate the provided documentation section on each criterion using a 1-5 scale.

Criteria:
1. SUMMARY_QUALITY: Does the opening accurately capture the section's purpose? (1=misleading, 5=perfect)
2. DEPTH: Does it explain WHY things are designed this way, not just WHAT exists? (1=surface-only, 5=deep insight)
3. ACTIONABILITY: Could a new developer use this to start contributing? (1=useless, 5=immediately actionable)
4. EXAMPLES: Are code references, file paths, or usage examples provided? (1=none, 5=comprehensive)

Respond with ONLY a JSON object:
{"summary_quality": N, "depth": N, "actionability": N, "examples": N}

Where N is an integer 1-5. No explanation, just the JSON."#;

        let mut total_score = 0.0f64;
        let mut sections_rated = 0usize;

        for (section_name, content) in doc_sections {
            // Skip very short sections.
            if content.len() < 100 {
                continue;
            }

            // Truncate to first 4000 chars to keep prompt size reasonable.
            let excerpt: String = content.chars().take(4000).collect();
            let user_prompt = format!(
                "Rate this documentation section titled '{}':\n\n{}",
                section_name, excerpt
            );

            match client
                .chat(
                    &config.llm.model_efficient,
                    system_prompt,
                    &user_prompt,
                    &config.llm,
                )
                .await
            {
                Ok(response) => {
                    if let Some(avg) = parse_helpfulness_scores(&response) {
                        total_score += avg;
                        sections_rated += 1;
                    } else {
                        findings.push(Finding {
                            severity: Severity::Info,
                            category: "helpfulness".to_string(),
                            message: format!(
                                "Could not parse LLM rating for section '{}'",
                                section_name
                            ),
                            section: Some(section_name.clone()),
                        });
                    }
                }
                Err(e) => {
                    findings.push(Finding {
                        severity: Severity::Warning,
                        category: "helpfulness".to_string(),
                        message: format!(
                            "LLM helpfulness check failed for '{}': {}",
                            section_name, e
                        ),
                        section: Some(section_name.clone()),
                    });
                }
            }
        }

        if sections_rated == 0 {
            // Distinguish "no eligible sections" from "all LLM calls failed"
            let eligible_sections = doc_sections.iter().filter(|(_, c)| c.len() >= 100).count();
            if eligible_sections > 0 {
                findings.push(Finding {
                    severity: Severity::Warning,
                    category: "helpfulness".to_string(),
                    message: format!(
                        "All {} eligible sections failed LLM helpfulness evaluation; defaulting to 1.0",
                        eligible_sections
                    ),
                    section: None,
                });
            }
            return 1.0;
        }

        let avg_score = total_score / sections_rated as f64;
        // Normalize from 1-5 scale to 0.0-1.0.
        let normalized = (avg_score - 1.0) / 4.0;

        findings.push(Finding {
            severity: Severity::Info,
            category: "helpfulness".to_string(),
            message: format!(
                "LLM-as-judge rated {} sections, average {:.1}/5 ({:.0}%)",
                sections_rated,
                avg_score,
                normalized * 100.0
            ),
            section: None,
        });

        normalized.clamp(0.0, 1.0)
    }
}

// ---------------------------------------------------------------------------
// Helpfulness scoring helpers (module-level so they are testable without
// constructing a ContentValidator).
// ---------------------------------------------------------------------------

/// Parse the four sub-scores from the LLM's JSON response.
///
/// Accepts raw JSON, JSON wrapped in a markdown code fence, or JSON embedded
/// anywhere in a prose response.  Returns the arithmetic mean as a value in
/// `1.0..=5.0`, or `None` when fewer than two valid scores can be extracted.
///
/// # Examples
///
/// ```rust,ignore
/// let avg = parse_helpfulness_scores(r#"{"summary_quality":4,"depth":3,"actionability":5,"examples":4}"#);
/// assert_eq!(avg, Some(4.0));
/// ```
pub(crate) fn parse_helpfulness_scores(response: &str) -> Option<f64> {
    // 1. Try direct JSON parse.
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(response) {
        return extract_scores_from_json(&v);
    }

    // 2. Try extracting JSON from a markdown code fence.
    let json_re = Regex::new(r"```(?:json)?\s*(\{[\s\S]*?\})\s*```").ok()?;
    if let Some(cap) = json_re.captures(response)
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(&cap[1])
    {
        return extract_scores_from_json(&v);
    }

    // 3. Find the first `{...}` block anywhere in the response.
    if let Some(start) = response.find('{')
        && let Some(end_offset) = response[start..].find('}')
        && let Ok(v) =
            serde_json::from_str::<serde_json::Value>(&response[start..=start + end_offset])
    {
        return extract_scores_from_json(&v);
    }

    None
}

/// Extract the four helpfulness sub-scores from a parsed JSON value.
///
/// Scores outside the valid `1..=5` range are silently ignored.
/// Returns `None` when fewer than two valid scores are found.
pub(crate) fn extract_scores_from_json(v: &serde_json::Value) -> Option<f64> {
    let keys = ["summary_quality", "depth", "actionability", "examples"];
    let mut sum = 0.0f64;
    let mut count = 0usize;

    for key in &keys {
        if let Some(score) = v.get(key).and_then(|s| s.as_f64())
            && (1.0..=5.0).contains(&score)
        {
            sum += score;
            count += 1;
        }
    }

    if count >= 2 {
        Some(sum / count as f64)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------

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
        assert!(findings.iter().any(|f| f.message.contains("too short")));
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
            coherence_score: 0.9,
            helpfulness_score: 0.8,
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

    // -----------------------------------------------------------------------
    // check_symbol_coverage tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_symbol_coverage_full() {
        use crate::types::code::{CodeDossier, CodeInsight};
        let docs = vec![(
            "Overview".to_string(),
            "The system uses `Config` and calls `execute` to run.".to_string(),
        )];
        let insights = vec![CodeInsight {
            code_dossier: CodeDossier {
                name: "config.rs".to_string(),
                functions: vec!["execute".to_string()],
                interfaces: vec!["Config".to_string()],
                ..Default::default()
            },
            ..Default::default()
        }];
        let mut findings = Vec::new();
        let score = ContentValidator::check_symbol_coverage(&docs, Some(&insights), &mut findings);
        assert!(
            (score - 1.0).abs() < f64::EPSILON,
            "expected 1.0, got {score}"
        );
        assert!(
            findings.is_empty(),
            "expected no findings, got {findings:?}"
        );
    }

    #[test]
    fn test_symbol_coverage_partial() {
        use crate::types::code::{CodeDossier, CodeInsight};
        let docs = vec![(
            "Overview".to_string(),
            "The `Config` struct controls behavior.".to_string(),
        )];
        let insights = vec![CodeInsight {
            code_dossier: CodeDossier {
                name: "config.rs".to_string(),
                functions: vec!["execute".to_string(), "validate".to_string()],
                interfaces: vec!["Config".to_string()],
                ..Default::default()
            },
            ..Default::default()
        }];
        let mut findings = Vec::new();
        let score = ContentValidator::check_symbol_coverage(&docs, Some(&insights), &mut findings);
        // 1 out of 3 symbols documented → ~0.333
        assert!(score > 0.3 && score < 0.4, "expected ~0.333, got {score}");
        assert!(
            !findings.is_empty(),
            "expected at least one finding for undocumented symbols"
        );
    }

    #[test]
    fn test_symbol_coverage_no_insights() {
        let docs = vec![("Overview".to_string(), "Hello".to_string())];
        let mut findings = Vec::new();
        let score = ContentValidator::check_symbol_coverage(&docs, None, &mut findings);
        assert!(
            (score - 1.0).abs() < f64::EPSILON,
            "expected 1.0 when no insights, got {score}"
        );
        assert!(findings.is_empty());
    }

    #[test]
    fn test_symbol_coverage_empty_insights_slice() {
        let docs = vec![("Overview".to_string(), "Hello".to_string())];
        let mut findings = Vec::new();
        let score = ContentValidator::check_symbol_coverage(&docs, Some(&[]), &mut findings);
        assert!(
            (score - 1.0).abs() < f64::EPSILON,
            "expected 1.0 for empty insights, got {score}"
        );
    }

    #[test]
    fn test_symbol_coverage_detailed_interfaces() {
        use crate::types::code::{CodeDossier, CodeInsight, InterfaceInfo};
        let docs = vec![(
            "Arch".to_string(),
            "The `MyTrait` interface is central.".to_string(),
        )];
        let insights = vec![CodeInsight {
            code_dossier: CodeDossier {
                name: "lib.rs".to_string(),
                ..Default::default()
            },
            interfaces: vec![
                InterfaceInfo {
                    name: "MyTrait".to_string(),
                    ..Default::default()
                },
                InterfaceInfo {
                    name: "HiddenImpl".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }];
        let mut findings = Vec::new();
        let score = ContentValidator::check_symbol_coverage(&docs, Some(&insights), &mut findings);
        // 1 out of 2 symbols documented → 0.5
        assert!(
            (score - 0.5).abs() < f64::EPSILON,
            "expected 0.5, got {score}"
        );
    }

    // -----------------------------------------------------------------------
    // check_coherence tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_coherence_consistent() {
        let docs = vec![
            (
                "A".to_string(),
                "The `Config` struct and `Config` usage.".to_string(),
            ),
            ("B".to_string(), "Uses `Config` throughout.".to_string()),
        ];
        let mut findings = Vec::new();
        let score = ContentValidator::check_coherence(&docs, &mut findings);
        assert!(
            (score - 1.0).abs() < f64::EPSILON,
            "expected 1.0 for consistent terminology, got {score}"
        );
        assert!(
            findings.is_empty(),
            "expected no findings, got {findings:?}"
        );
    }

    #[test]
    fn test_coherence_inconsistent() {
        let docs = vec![
            ("A".to_string(), "The `CodeInsight` type.".to_string()),
            (
                "B".to_string(),
                "Uses `code_insight` for analysis.".to_string(),
            ),
        ];
        let mut findings = Vec::new();
        let score = ContentValidator::check_coherence(&docs, &mut findings);
        assert!(
            score < 1.0,
            "expected score < 1.0 for inconsistency, got {score}"
        );
        assert!(
            findings.iter().any(|f| f.category == "coherence"),
            "expected at least one coherence finding"
        );
    }

    #[test]
    fn test_coherence_no_terms() {
        let docs = vec![(
            "A".to_string(),
            "This is a plain text document.".to_string(),
        )];
        let mut findings = Vec::new();
        let score = ContentValidator::check_coherence(&docs, &mut findings);
        assert!(
            (score - 1.0).abs() < f64::EPSILON,
            "expected 1.0 when no backtick terms, got {score}"
        );
        assert!(findings.is_empty());
    }

    #[test]
    fn test_coherence_many_inconsistencies_cap() {
        // Generate more than 10 inconsistent term groups to verify the
        // "and N more" summary finding is emitted.
        let mut content_a = String::new();
        let mut content_b = String::new();
        for i in 0..15 {
            content_a.push_str(&format!("`Term{i}Camel` "));
            content_b.push_str(&format!("`term{i}_camel` "));
        }
        let docs = vec![("A".to_string(), content_a), ("B".to_string(), content_b)];
        let mut findings = Vec::new();
        let score = ContentValidator::check_coherence(&docs, &mut findings);
        assert!(score < 1.0);
        // Should have exactly 10 individual coherence findings + 1 summary
        let coherence_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.category == "coherence")
            .collect();
        assert_eq!(
            coherence_findings.len(),
            11,
            "expected 10 individual + 1 summary finding, got {}",
            coherence_findings.len()
        );
        assert!(
            coherence_findings
                .last()
                .unwrap()
                .message
                .contains("more terminology"),
            "last finding should be summary"
        );
    }

    // -----------------------------------------------------------------------
    // parse_helpfulness_scores / extract_scores_from_json tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_helpfulness_scores_valid() {
        let response = r#"{"summary_quality": 4, "depth": 3, "actionability": 5, "examples": 4}"#;
        let avg = parse_helpfulness_scores(response).unwrap();
        assert!((avg - 4.0).abs() < f64::EPSILON, "expected 4.0, got {avg}");
    }

    #[test]
    fn test_parse_helpfulness_scores_in_code_block() {
        let response = "Here are my ratings:\n```json\n{\"summary_quality\": 3, \"depth\": 4, \"actionability\": 2, \"examples\": 3}\n```";
        let avg = parse_helpfulness_scores(response).unwrap();
        assert!((avg - 3.0).abs() < f64::EPSILON, "expected 3.0, got {avg}");
    }

    #[test]
    fn test_parse_helpfulness_scores_invalid() {
        assert!(
            parse_helpfulness_scores("not json at all").is_none(),
            "plain text should return None"
        );
        assert!(
            parse_helpfulness_scores("{}").is_none(),
            "empty object should return None (fewer than 2 valid scores)"
        );
    }

    #[test]
    fn test_parse_helpfulness_scores_partial_keys() {
        // Only 2 valid keys — should still produce a result.
        let response = r#"{"summary_quality": 4, "depth": 3}"#;
        let avg = parse_helpfulness_scores(response).unwrap();
        assert!((avg - 3.5).abs() < f64::EPSILON, "expected 3.5, got {avg}");
    }

    #[test]
    fn test_parse_helpfulness_scores_out_of_range() {
        // Scores outside 1-5 are ignored; only depth=3 and actionability=4 are valid.
        let response = r#"{"summary_quality": 10, "depth": 3, "actionability": 4, "examples": 0}"#;
        let avg = parse_helpfulness_scores(response).unwrap();
        assert!(
            (avg - 3.5).abs() < f64::EPSILON,
            "expected 3.5 (only depth=3 + actionability=4), got {avg}"
        );
    }

    #[test]
    fn test_extract_scores_from_json_all_valid() {
        let v: serde_json::Value = serde_json::json!({
            "summary_quality": 4,
            "depth": 3,
            "actionability": 5,
            "examples": 4
        });
        let avg = extract_scores_from_json(&v).unwrap();
        assert!(
            (avg - 4.0).abs() < f64::EPSILON,
            "expected (4+3+5+4)/4=4.0, got {avg}"
        );
    }

    #[test]
    fn test_extract_scores_from_json_only_one_valid_returns_none() {
        let v: serde_json::Value = serde_json::json!({"summary_quality": 3});
        assert!(
            extract_scores_from_json(&v).is_none(),
            "single valid score is not enough"
        );
    }

    #[test]
    fn test_parse_helpfulness_scores_embedded_in_prose() {
        // JSON embedded after prose text — heuristic extraction via first '{...}'.
        let response = r#"Sure! Here is my evaluation: {"summary_quality": 2, "depth": 5, "actionability": 3, "examples": 4} I hope that helps."#;
        let avg = parse_helpfulness_scores(response).unwrap();
        // (2+5+3+4)/4 = 3.5
        assert!((avg - 3.5).abs() < f64::EPSILON, "expected 3.5, got {avg}");
    }

    #[test]
    fn test_symbol_coverage_short_name_word_boundary() {
        // "id" should NOT match "considered" — word boundary enforcement
        let docs = vec![(
            "Overview".to_string(),
            "This is considered important for the system.".to_string(),
        )];
        let insights = vec![crate::types::code::CodeInsight {
            code_dossier: crate::types::code::CodeDossier {
                functions: vec!["id".to_string()],
                interfaces: vec![],
                ..Default::default()
            },
            interfaces: vec![],
            ..Default::default()
        }];
        let mut findings = Vec::new();
        let score = ContentValidator::check_symbol_coverage(&docs, Some(&insights), &mut findings);
        // "id" should NOT be found via substring in "considered"
        assert!(
            score < 1.0,
            "Short symbol 'id' should not match 'considered' via substring"
        );
        assert!(!findings.is_empty());
    }

    #[test]
    fn test_symbol_coverage_short_name_exact_word_match() {
        // "id" SHOULD match when it appears as a standalone word
        let docs = vec![(
            "Overview".to_string(),
            "The id field is the primary key.".to_string(),
        )];
        let insights = vec![crate::types::code::CodeInsight {
            code_dossier: crate::types::code::CodeDossier {
                functions: vec!["id".to_string()],
                interfaces: vec![],
                ..Default::default()
            },
            interfaces: vec![],
            ..Default::default()
        }];
        let mut findings = Vec::new();
        let score = ContentValidator::check_symbol_coverage(&docs, Some(&insights), &mut findings);
        assert!(
            (score - 1.0).abs() < f64::EPSILON,
            "Short symbol 'id' should match standalone word 'id'"
        );
    }

    #[test]
    fn test_coherence_rust_casing_convention_excluded() {
        // to_string vs ToString should NOT be flagged as inconsistency
        let docs = vec![(
            "Overview".to_string(),
            "Use `to_string` for conversion. The `ToString` trait is also useful.".to_string(),
        )];
        let mut findings = Vec::new();
        let score = ContentValidator::check_coherence(&docs, &mut findings);
        // Should be perfect since snake_case vs PascalCase of same term is expected in Rust
        let coherence_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.category == "coherence")
            .collect();
        assert!(
            coherence_findings.is_empty(),
            "Rust casing convention (to_string vs ToString) should not be flagged: {:?}",
            coherence_findings
        );
        assert!(
            (score - 1.0).abs() < 0.01,
            "Score should be ~1.0 but got {}",
            score
        );
    }
}
