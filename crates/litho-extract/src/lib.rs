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
use std::collections::HashMap;
use std::path::Path;

/// Extract a project using the default [`LithoConfig`].
///
/// # Errors
///
/// Propagates I/O errors encountered while walking the project tree.
pub async fn extract(project_path: &Path) -> anyhow::Result<ExtractedCodebase> {
    extract_with_config(project_path, &LithoConfig::default()).await
}

/// Extract a project with explicit configuration.
///
/// # Errors
///
/// Propagates I/O errors encountered while walking the project tree.
pub async fn extract_with_config(
    project_path: &Path,
    config: &LithoConfig,
) -> anyhow::Result<ExtractedCodebase> {
    let discovered = discovery::discover_files(
        project_path,
        &config.excluded_dirs,
        &config.excluded_extensions,
        config.max_file_size,
    );

    let mut files = Vec::new();
    let mut all_deps: Vec<(String, Vec<Dependency>)> = Vec::new();
    let mut lang_counts: HashMap<String, usize> = HashMap::new();
    let mut total_loc: usize = 0;

    for df in &discovered {
        let content = match std::fs::read_to_string(&df.path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let interfaces = extract_interfaces_for_language(&df.language, &content, &df.path);
        let deps = extract_dependencies_for_language(&df.language, &content, &df.path);

        let has_main = interfaces
            .iter()
            .any(|i| i.name == "main" && i.kind == "function");
        let classification = classify::classify_file(&df.path, has_main);

        let mut cx = complexity::compute_complexity(&content, &df.language);
        cx.functions = interfaces
            .iter()
            .filter(|i| i.kind == "function")
            .count();
        cx.classes = interfaces
            .iter()
            .filter(|i| matches!(i.kind.as_str(), "struct" | "class" | "trait" | "enum"))
            .count();

        total_loc += cx.lines_of_code;
        *lang_counts
            .entry(format!("{:?}", df.language))
            .or_insert(0) += 1;

        let path_str = df.path.to_string_lossy().to_string();
        all_deps.push((path_str, deps.clone()));

        let loc = complexity::total_lines(&content);

        files.push(ExtractedFile {
            path: df.path.clone(),
            language: df.language.clone(),
            classification,
            complexity: cx,
            dependencies: deps,
            interfaces,
            lines_of_code: loc,
            size_bytes: df.size,
        });
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

fn extract_interfaces_for_language(
    lang: &Language,
    content: &str,
    path: &Path,
) -> Vec<Interface> {
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
