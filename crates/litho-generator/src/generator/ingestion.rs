use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::Result;
use litho_core::config::{ExtractBackend, LithoConfig};
use litho_extract::extract_with_config;
use serde::{Deserialize, Serialize};

use crate::types::code::CodeInsight;
use crate::types::code_releationship::RelationshipAnalysis;
use crate::types::project_structure::ProjectStructure;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IngestionDag {
    pub generated_at: String,
    pub root_path: String,
    pub nodes: Vec<IngestionDagNode>,
    pub edges: Vec<IngestionDagEdge>,
    pub rag_chunks: Vec<IngestionRagChunk>,
    pub ast_files_indexed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IngestionDagNode {
    pub path: String,
    pub is_core: bool,
    pub file_size: u64,
    pub complexity_score: f64,
    pub content_hash: Option<String>,
    pub symbols: Vec<String>,
    pub dependencies: Vec<String>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IngestionDagEdge {
    pub from: String,
    pub to: String,
    pub edge_type: String,
    pub is_external: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IngestionRagChunk {
    pub id: String,
    pub path: String,
    pub summary: String,
    pub symbols: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct AstFileSummary {
    symbols: Vec<String>,
    deps: Vec<String>,
}

pub async fn build_ingestion_dag(
    project_structure: &ProjectStructure,
    insights: &[CodeInsight],
    relationships: &RelationshipAnalysis,
) -> Result<IngestionDag> {
    let root = project_structure.root_path.clone();
    let ast_index = tokio::task::spawn_blocking(move || build_ast_index(&root))
        .await
        .ok()
        .and_then(|result| result.ok())
        .unwrap_or_default();

    let mut insight_by_path: HashMap<String, &CodeInsight> = HashMap::new();
    for insight in insights {
        let rel = normalize_to_repo_path(
            &project_structure.root_path,
            &insight.code_dossier.file_path,
        );
        insight_by_path.insert(rel, insight);
    }

    let mut nodes = Vec::with_capacity(project_structure.files.len());
    let mut node_paths = HashSet::new();
    for file in &project_structure.files {
        let path = normalize_to_repo_path(&project_structure.root_path, &file.path);
        node_paths.insert(path.clone());

        let insight = insight_by_path
            .get(&path)
            .copied()
            .or_else(|| insights.iter().find(|v| v.code_dossier.name == file.name));
        let ast = ast_index.get(&path);

        let mut symbols_set: HashSet<String> = HashSet::new();
        if let Some(info) = insight {
            symbols_set.extend(
                info.code_dossier
                    .functions
                    .iter()
                    .filter(|v| !v.trim().is_empty())
                    .cloned(),
            );
            symbols_set.extend(
                info.code_dossier
                    .interfaces
                    .iter()
                    .filter(|v| !v.trim().is_empty())
                    .cloned(),
            );
            symbols_set.extend(
                info.interfaces
                    .iter()
                    .filter(|v| !v.name.trim().is_empty())
                    .map(|v| v.name.clone()),
            );
        }
        if let Some(ast) = ast {
            symbols_set.extend(ast.symbols.iter().filter(|v| !v.trim().is_empty()).cloned());
        }

        let mut deps_set: HashSet<String> = HashSet::new();
        if let Some(info) = insight {
            deps_set.extend(info.dependencies.iter().filter_map(|d| {
                d.path
                    .as_ref()
                    .filter(|v| !v.trim().is_empty())
                    .cloned()
                    .or_else(|| {
                        if d.name.trim().is_empty() {
                            None
                        } else {
                            Some(d.name.clone())
                        }
                    })
            }));
        }
        if let Some(ast) = ast {
            deps_set.extend(ast.deps.iter().filter(|v| !v.trim().is_empty()).cloned());
        }

        let abs = project_structure.root_path.join(&file.path);
        let content_hash = std::fs::read(&abs)
            .ok()
            .map(|bytes| blake3::hash(&bytes).to_hex().to_string());

        let mut symbols: Vec<String> = symbols_set.into_iter().collect();
        symbols.sort();
        let mut dependencies: Vec<String> = deps_set.into_iter().collect();
        dependencies.sort();

        nodes.push(IngestionDagNode {
            path,
            is_core: file.is_core,
            file_size: file.size,
            complexity_score: file.complexity_score,
            content_hash,
            symbols,
            dependencies,
            summary: insight.map(|v| {
                if !v.detailed_description.trim().is_empty() {
                    v.detailed_description.clone()
                } else {
                    v.code_dossier.description.clone().unwrap_or_default()
                }
            }),
        });
    }

    let mut edges = Vec::new();
    for node in &nodes {
        for dep in &node.dependencies {
            edges.push(IngestionDagEdge {
                from: node.path.clone(),
                to: dep.clone(),
                edge_type: "dependency".to_string(),
                is_external: !node_paths.contains(dep),
            });
        }
    }
    for rel in &relationships.core_dependencies {
        let from = resolve_node_from_label(&rel.from, &nodes).unwrap_or_else(|| rel.from.clone());
        let to = resolve_node_from_label(&rel.to, &nodes).unwrap_or_else(|| rel.to.clone());
        edges.push(IngestionDagEdge {
            from,
            to,
            edge_type: rel.dependency_type.as_str().to_string(),
            is_external: false,
        });
    }

    let rag_chunks = nodes
        .iter()
        .filter(|n| n.is_core || !n.symbols.is_empty())
        .take(256)
        .enumerate()
        .map(|(idx, node)| IngestionRagChunk {
            id: format!("dag-{}", idx + 1),
            path: node.path.clone(),
            summary: build_rag_summary(node),
            symbols: node.symbols.iter().take(24).cloned().collect(),
        })
        .collect();

    Ok(IngestionDag {
        generated_at: chrono::Utc::now().to_rfc3339(),
        root_path: project_structure.root_path.display().to_string(),
        nodes,
        edges,
        rag_chunks,
        ast_files_indexed: ast_index.len(),
    })
}

pub fn persist_dag(path: &Path, dag: &IngestionDag) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(dag)?)?;
    Ok(())
}

fn build_ast_index(root: &Path) -> Result<HashMap<String, AstFileSummary>> {
    let mut cfg = LithoConfig {
        project_path: root.to_path_buf(),
        extract_backend: ExtractBackend::Auto,
        ..LithoConfig::default()
    };
    cfg.excluded_dirs.extend([
        ".litho".to_string(),
        "target".to_string(),
        "node_modules".to_string(),
    ]);
    cfg.excluded_dirs.sort();
    cfg.excluded_dirs.dedup();

    let extracted = extract_with_config(root, &cfg)?;
    let mut out = HashMap::new();
    for file in extracted.files {
        let path = normalize_to_repo_path(root, &file.path);
        let symbols = file.interfaces.into_iter().map(|i| i.name).collect();
        let deps = file.dependencies.into_iter().map(|d| d.target).collect();
        out.insert(path, AstFileSummary { symbols, deps });
    }
    Ok(out)
}

fn normalize_to_repo_path(root: &Path, path: &Path) -> String {
    let rel = if path.is_absolute() {
        path.strip_prefix(root).unwrap_or(path)
    } else {
        path
    };
    rel.to_string_lossy().replace('\\', "/")
}

fn resolve_node_from_label(label: &str, nodes: &[IngestionDagNode]) -> Option<String> {
    let needle = label.to_ascii_lowercase();
    nodes
        .iter()
        .find(|node| {
            let path = node.path.to_ascii_lowercase();
            path.ends_with(&needle)
                || path.contains(&format!("/{needle}"))
                || PathBuf::from(&node.path)
                    .file_stem()
                    .and_then(|v| v.to_str())
                    .map(|v| v.eq_ignore_ascii_case(label))
                    .unwrap_or(false)
        })
        .map(|node| node.path.clone())
}

fn build_rag_summary(node: &IngestionDagNode) -> String {
    let mut parts = Vec::new();
    if let Some(summary) = &node.summary
        && !summary.trim().is_empty()
    {
        parts.push(summary.trim().to_string());
    }
    if !node.symbols.is_empty() {
        let symbol_sample = node.symbols.iter().take(8).cloned().collect::<Vec<_>>();
        parts.push(format!("Symbols: {}", symbol_sample.join(", ")));
    }
    if !node.dependencies.is_empty() {
        let dep_sample = node
            .dependencies
            .iter()
            .take(6)
            .cloned()
            .collect::<Vec<_>>();
        parts.push(format!("Dependencies: {}", dep_sample.join(", ")));
    }
    if parts.is_empty() {
        format!(
            "File {} ({} bytes, complexity {:.2})",
            node.path, node.file_size, node.complexity_score
        )
    } else {
        parts.join(" | ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_node_from_label_matches_file_stem() {
        let nodes = vec![IngestionDagNode {
            path: "src/parser.rs".to_string(),
            ..Default::default()
        }];
        let matched = resolve_node_from_label("parser", &nodes);
        assert_eq!(matched.as_deref(), Some("src/parser.rs"));
    }

    #[test]
    fn rag_summary_includes_symbols_and_deps() {
        let node = IngestionDagNode {
            path: "src/main.rs".to_string(),
            symbols: vec!["main".to_string(), "run".to_string()],
            dependencies: vec!["std::env".to_string()],
            summary: Some("Entry point".to_string()),
            ..Default::default()
        };
        let summary = build_rag_summary(&node);
        assert!(summary.contains("Entry point"));
        assert!(summary.contains("Symbols: main, run"));
        assert!(summary.contains("Dependencies: std::env"));
    }
}
