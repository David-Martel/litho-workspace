pub mod classify;
pub mod complexity;
pub mod discovery;
pub mod extractors;
pub mod graph;
pub mod parser;
pub mod types;

pub use litho_core::types::ExtractedCodebase;

use litho_core::config::LithoConfig;
use litho_core::types::{Dependency, ExtractedFile, Interface, Language, ProjectStats};
use rayon::prelude::*;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

const AST_WALKER_BATCH_SIZE: usize = 64;

/// Extract a project using the default [`LithoConfig`].
///
/// # Errors
///
/// Propagates I/O errors encountered while walking the project tree.
pub fn extract(project_path: &Path) -> anyhow::Result<ExtractedCodebase> {
    extract_with_config(project_path, &LithoConfig::default())
}

/// Extract a project with explicit configuration.
///
/// # Errors
///
/// Propagates I/O errors encountered while walking the project tree.
pub fn extract_with_config(
    project_path: &Path,
    config: &LithoConfig,
) -> anyhow::Result<ExtractedCodebase> {
    let mut discovered = discovery::discover_files(
        project_path,
        &config.excluded_dirs,
        &config.excluded_extensions,
        config.max_file_size,
    );
    // Keep processing stable across runs even with parallel analysis.
    discovered.sort_by(|a, b| a.path.cmp(&b.path));

    let mut files = Vec::new();
    let mut all_deps: Vec<(String, Vec<Dependency>)> = Vec::new();
    let mut lang_counts: HashMap<String, usize> = HashMap::new();
    let mut total_loc: usize = 0;

    let batches = build_ast_walker_batches(&discovered, project_path, AST_WALKER_BATCH_SIZE);
    let mut analyzed: Vec<AnalyzedFile> = Vec::new();
    for batch in batches {
        let batch_files: Vec<_> = batch
            .iter()
            .map(|df| (df.path.clone(), df.language.clone()))
            .collect();
        let ast_grep_hints = match parser::collect_ast_grep_hints_for_files(&batch_files, config) {
            Ok(hints) => hints,
            Err(err) => {
                if !matches!(
                    config.extract_backend,
                    litho_core::config::ExtractBackend::TreeSitter
                ) {
                    eprintln!(
                        "⚠️  ast-grep batch mode unavailable for current batch, falling back to tree-sitter: {}",
                        err
                    );
                }
                HashMap::new()
            }
        };

        let mut batch_results: Vec<AnalyzedFile> = batch
            .par_iter()
            .filter_map(|df| analyze_discovered_file(df, ast_grep_hints.get(&df.path)))
            .collect();
        analyzed.append(&mut batch_results);
    }
    analyzed.sort_by(|a, b| a.extracted.path.cmp(&b.extracted.path));

    for item in analyzed {
        total_loc += item.extracted.complexity.lines_of_code;
        *lang_counts.entry(item.language_label).or_insert(0) += 1;
        all_deps.push(item.dep_entry);
        files.push(item.extracted);
    }

    let dep_graph = graph::build_dependency_graph(&all_deps);

    let mut top_complex: Vec<_> = files
        .iter()
        .map(|f| (f.path.clone(), f.complexity.cyclomatic))
        .collect();
    top_complex.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let top_complex_files: Vec<_> = top_complex
        .iter()
        .take(10)
        .map(|(p, _)| p.clone())
        .collect();

    let project_name = project_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    Ok(ExtractedCodebase {
        project_name,
        files,
        dependency_graph: dep_graph,
        statistics: ProjectStats {
            total_files: discovered.len(),
            total_loc,
            languages: lang_counts,
            top_complex_files,
        },
    })
}

struct AnalyzedFile {
    extracted: ExtractedFile,
    dep_entry: (String, Vec<Dependency>),
    language_label: String,
}

fn analyze_discovered_file(
    df: &discovery::DiscoveredFile,
    ast_grep_hints: Option<&parser::AstGrepFileHints>,
) -> Option<AnalyzedFile> {
    let content = std::fs::read_to_string(&df.path).ok()?;

    let mut interfaces = extract_interfaces_for_language(&df.language, &content, &df.path);
    let mut deps = extract_dependencies_for_language(&df.language, &content, &df.path);
    if let Some(hints) = ast_grep_hints {
        apply_ast_grep_hints(&mut interfaces, &mut deps, hints, &df.path);
    }

    let has_main = interfaces
        .iter()
        .any(|i| i.name == "main" && i.kind == "function");
    let classification = classify::classify_file(&df.path, has_main);

    let mut cx = complexity::compute_complexity(&content, &df.language);
    cx.functions = interfaces.iter().filter(|i| i.kind == "function").count();
    cx.classes = interfaces
        .iter()
        .filter(|i| matches!(i.kind.as_str(), "struct" | "class" | "trait" | "enum"))
        .count();

    let loc = complexity::total_lines(&content);
    let path_str = df.path.to_string_lossy().to_string();

    Some(AnalyzedFile {
        extracted: ExtractedFile {
            path: df.path.clone(),
            language: df.language.clone(),
            classification,
            complexity: cx,
            dependencies: deps.clone(),
            interfaces,
            lines_of_code: loc,
            size_bytes: df.size,
        },
        dep_entry: (path_str, deps),
        language_label: format!("{:?}", df.language),
    })
}

fn apply_ast_grep_hints(
    interfaces: &mut Vec<Interface>,
    deps: &mut Vec<Dependency>,
    hints: &parser::AstGrepFileHints,
    file_path: &Path,
) {
    for hint in &hints.interfaces {
        let exists = interfaces.iter().any(|interface| {
            interface.name == hint.name
                && interface.kind == hint.kind
                && interface.line == hint.line
        });
        if !exists {
            interfaces.push(Interface {
                name: hint.name.clone(),
                kind: hint.kind.clone(),
                visibility: hint.visibility.clone(),
                signature: hint.signature.clone(),
                line: hint.line,
            });
        }
    }

    let source = file_path.to_string_lossy().to_string();
    for hint in &hints.dependencies {
        let exists = deps
            .iter()
            .any(|dep| dep.target == hint.target && dep.kind == hint.kind);
        if !exists {
            deps.push(Dependency {
                source: source.clone(),
                target: hint.target.clone(),
                kind: hint.kind.clone(),
            });
        }
    }
}

fn build_ast_walker_batches(
    discovered: &[discovery::DiscoveredFile],
    project_root: &Path,
    max_batch_size: usize,
) -> Vec<Vec<discovery::DiscoveredFile>> {
    let mut grouped: BTreeMap<(String, String), Vec<discovery::DiscoveredFile>> = BTreeMap::new();
    for df in discovered {
        let relative = df.path.strip_prefix(project_root).unwrap_or(&df.path);
        let top_level = relative
            .components()
            .next()
            .and_then(|c| c.as_os_str().to_str())
            .unwrap_or("")
            .to_string();
        let key = (top_level, format!("{:?}", df.language));
        grouped.entry(key).or_default().push(df.clone());
    }

    let mut batches = Vec::new();
    for (_key, group) in grouped {
        if group.len() <= max_batch_size {
            batches.push(group);
            continue;
        }
        let mut start = 0usize;
        while start < group.len() {
            let end = (start + max_batch_size).min(group.len());
            batches.push(group[start..end].to_vec());
            start = end;
        }
    }
    batches
}

fn extract_interfaces_for_language(lang: &Language, content: &str, path: &Path) -> Vec<Interface> {
    use extractors::Extractor as _;
    match lang {
        Language::Rust => extractors::rust::RustExtractor::new().extract_interfaces(content, path),
        Language::TypeScript => {
            extractors::typescript::TypeScriptExtractor::new().extract_interfaces(content, path)
        }
        Language::Python => {
            extractors::python::PythonExtractor::new().extract_interfaces(content, path)
        }
        Language::CSharp => {
            extractors::csharp::CSharpExtractor::new().extract_interfaces(content, path)
        }
        _ => vec![],
    }
}

fn extract_dependencies_for_language(
    lang: &Language,
    content: &str,
    path: &Path,
) -> Vec<Dependency> {
    use extractors::Extractor as _;
    match lang {
        Language::Rust => {
            extractors::rust::RustExtractor::new().extract_dependencies(content, path)
        }
        Language::TypeScript => {
            extractors::typescript::TypeScriptExtractor::new().extract_dependencies(content, path)
        }
        Language::Python => {
            extractors::python::PythonExtractor::new().extract_dependencies(content, path)
        }
        Language::CSharp => {
            extractors::csharp::CSharpExtractor::new().extract_dependencies(content, path)
        }
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_discovered(path: &str, language: Language) -> discovery::DiscoveredFile {
        discovery::DiscoveredFile {
            path: PathBuf::from(path),
            language,
            size: 10,
        }
    }

    #[test]
    fn ast_walker_batches_group_by_top_level_and_language() {
        let root = Path::new("repo");
        let discovered = vec![
            make_discovered("repo/src/a.rs", Language::Rust),
            make_discovered("repo/src/b.rs", Language::Rust),
            make_discovered("repo/scripts/a.py", Language::Python),
            make_discovered("repo/src/app.ts", Language::TypeScript),
        ];

        let batches = build_ast_walker_batches(&discovered, root, 64);
        // Expect separate groups: (src,Rust), (src,TypeScript), (scripts,Python)
        assert_eq!(batches.len(), 3);
        assert_eq!(batches.iter().map(|b| b.len()).sum::<usize>(), 4);
    }

    #[test]
    fn ast_walker_batches_split_large_groups() {
        let root = Path::new("repo");
        let mut discovered = Vec::new();
        for i in 0..5 {
            discovered.push(make_discovered(
                &format!("repo/src/file{i}.rs"),
                Language::Rust,
            ));
        }

        let batches = build_ast_walker_batches(&discovered, root, 2);
        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0].len(), 2);
        assert_eq!(batches[1].len(), 2);
        assert_eq!(batches[2].len(), 1);
    }

    #[test]
    fn apply_ast_grep_hints_adds_missing_symbols_and_deps() {
        let mut interfaces = vec![Interface {
            name: "existing".to_string(),
            kind: "function".to_string(),
            visibility: "pub".to_string(),
            signature: "pub fn existing() {}".to_string(),
            line: 1,
        }];
        let mut deps = vec![Dependency {
            source: "repo/src/lib.rs".to_string(),
            target: "std::fmt".to_string(),
            kind: "use".to_string(),
        }];
        let hints = parser::AstGrepFileHints {
            interfaces: vec![parser::AstGrepInterfaceHint {
                name: "Settings".to_string(),
                kind: "struct".to_string(),
                visibility: "pub".to_string(),
                signature: "pub struct Settings {}".to_string(),
                line: 10,
            }],
            dependencies: vec![parser::AstGrepDependencyHint {
                target: "crate::config::Cfg".to_string(),
                kind: "use".to_string(),
            }],
        };

        apply_ast_grep_hints(
            &mut interfaces,
            &mut deps,
            &hints,
            Path::new("repo/src/lib.rs"),
        );

        assert!(interfaces.iter().any(|i| i.name == "Settings"));
        assert!(deps.iter().any(|d| d.target == "crate::config::Cfg"));
    }

    #[test]
    fn apply_ast_grep_hints_does_not_duplicate_existing_entries() {
        let mut interfaces = vec![Interface {
            name: "Settings".to_string(),
            kind: "struct".to_string(),
            visibility: "pub".to_string(),
            signature: "pub struct Settings {}".to_string(),
            line: 10,
        }];
        let mut deps = vec![Dependency {
            source: "repo/src/lib.rs".to_string(),
            target: "crate::config::Cfg".to_string(),
            kind: "use".to_string(),
        }];
        let hints = parser::AstGrepFileHints {
            interfaces: vec![parser::AstGrepInterfaceHint {
                name: "Settings".to_string(),
                kind: "struct".to_string(),
                visibility: "pub".to_string(),
                signature: "pub struct Settings {}".to_string(),
                line: 10,
            }],
            dependencies: vec![parser::AstGrepDependencyHint {
                target: "crate::config::Cfg".to_string(),
                kind: "use".to_string(),
            }],
        };

        apply_ast_grep_hints(
            &mut interfaces,
            &mut deps,
            &hints,
            Path::new("repo/src/lib.rs"),
        );

        assert_eq!(interfaces.len(), 1);
        assert_eq!(deps.len(), 1);
    }
}
