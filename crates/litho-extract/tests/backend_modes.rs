use litho_core::config::{ExtractBackend, LithoConfig};
use litho_extract::extract_with_config;
use rstest::rstest;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn write_sample_rust_project() -> TempDir {
    write_sample_project(
        "src/lib.rs",
        r#"
use std::path::PathBuf;

pub fn hello(name: &str) -> String {
    format!("Hello, {name}")
}

pub struct Settings {
    pub path: PathBuf,
}
"#,
    )
}

fn write_sample_typescript_project() -> TempDir {
    write_sample_project(
        "src/index.ts",
        r#"
import path from "path";

export function hello(name: string): string {
    return `Hello, ${name}`;
}

export class Settings {
    path: string;
}
"#,
    )
}

fn write_sample_python_project() -> TempDir {
    write_sample_project(
        "src/app.py",
        r#"
import pathlib

def hello(name: str) -> str:
    return f"Hello, {name}"

class Settings:
    path: pathlib.Path
"#,
    )
}

fn write_sample_csharp_project() -> TempDir {
    write_sample_project(
        "src/Program.cs",
        r#"
using System;

public static class Greeter
{
    public static string Hello(string name)
    {
        return $"Hello, {name}";
    }
}

public class Settings
{
    public string Path { get; set; } = string.Empty;
}
"#,
    )
}

fn write_sample_project(relative_path: &str, content: &str) -> TempDir {
    let temp = TempDir::new().unwrap();
    let src_dir = temp.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(
        src_dir.join(relative_path.strip_prefix("src/").unwrap_or(relative_path)),
        content,
    )
    .unwrap();
    temp
}

fn rel_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[derive(Debug, Serialize)]
struct SnapshotDependency {
    source: String,
    target: String,
    kind: String,
}

#[derive(Debug, Serialize)]
struct SnapshotInterface {
    name: String,
    kind: String,
    visibility: String,
    signature: String,
    line: usize,
}

#[derive(Debug, Serialize)]
struct SnapshotComplexity {
    cyclomatic: f64,
    lines_of_code: usize,
    functions: usize,
    classes: usize,
}

#[derive(Debug, Serialize)]
struct SnapshotFile {
    path: String,
    language: String,
    classification: String,
    complexity: SnapshotComplexity,
    dependencies: Vec<SnapshotDependency>,
    interfaces: Vec<SnapshotInterface>,
    lines_of_code: usize,
    size_bytes: u64,
}

#[derive(Debug, Serialize)]
struct SnapshotStats {
    total_files: usize,
    total_loc: usize,
    languages: BTreeMap<String, usize>,
    top_complex_files: Vec<String>,
}

#[derive(Debug, Serialize)]
struct SnapshotCodebase {
    project_name: String,
    files: Vec<SnapshotFile>,
    dependency_graph: BTreeMap<String, Vec<String>>,
    statistics: SnapshotStats,
}

fn snapshot_codebase(
    project_root: &Path,
    codebase: &litho_core::types::ExtractedCodebase,
) -> SnapshotCodebase {
    let mut dependency_graph = BTreeMap::new();
    for (source, targets) in &codebase.dependency_graph {
        let mut normalized_targets: Vec<String> = targets
            .iter()
            .map(|target| rel_path(project_root, Path::new(target)))
            .collect();
        normalized_targets.sort();
        normalized_targets.dedup();
        dependency_graph.insert(
            rel_path(project_root, Path::new(source)),
            normalized_targets,
        );
    }

    let files = codebase
        .files
        .iter()
        .map(|file| {
            let mut dependencies: Vec<SnapshotDependency> = file
                .dependencies
                .iter()
                .map(|dep| SnapshotDependency {
                    source: rel_path(project_root, Path::new(&dep.source)),
                    target: rel_path(project_root, Path::new(&dep.target)),
                    kind: dep.kind.clone(),
                })
                .collect();
            dependencies.sort_by(|a, b| a.target.cmp(&b.target).then(a.kind.cmp(&b.kind)));

            let mut interfaces: Vec<SnapshotInterface> = file
                .interfaces
                .iter()
                .map(|iface| SnapshotInterface {
                    name: iface.name.clone(),
                    kind: iface.kind.clone(),
                    visibility: iface.visibility.clone(),
                    signature: iface.signature.clone(),
                    line: iface.line,
                })
                .collect();
            interfaces.sort_by(|a, b| a.name.cmp(&b.name).then(a.line.cmp(&b.line)));

            SnapshotFile {
                path: rel_path(project_root, &file.path),
                language: format!("{:?}", file.language),
                classification: format!("{:?}", file.classification),
                complexity: SnapshotComplexity {
                    cyclomatic: file.complexity.cyclomatic,
                    lines_of_code: file.complexity.lines_of_code,
                    functions: file.complexity.functions,
                    classes: file.complexity.classes,
                },
                dependencies,
                interfaces,
                lines_of_code: file.lines_of_code,
                size_bytes: file.size_bytes,
            }
        })
        .collect();

    let mut languages = BTreeMap::new();
    for (language, count) in &codebase.statistics.languages {
        languages.insert(language.clone(), *count);
    }

    SnapshotCodebase {
        project_name: "sample-project".to_string(),
        files,
        dependency_graph,
        statistics: SnapshotStats {
            total_files: codebase.statistics.total_files,
            total_loc: codebase.statistics.total_loc,
            languages,
            top_complex_files: codebase
                .statistics
                .top_complex_files
                .iter()
                .map(|path| rel_path(project_root, path))
                .collect(),
        },
    }
}

#[test]
fn tree_sitter_backend_works_without_ast_grep_binary() {
    let temp = write_sample_rust_project();
    let cfg = LithoConfig {
        extract_backend: ExtractBackend::TreeSitter,
        ast_grep_binary: Some("definitely-missing-sg-bin".to_string()),
        ..LithoConfig::default()
    };

    let extracted = extract_with_config(temp.path(), &cfg).unwrap();
    assert_eq!(extracted.statistics.total_files, 1);
    assert!(
        extracted.files[0]
            .interfaces
            .iter()
            .any(|iface| iface.name == "hello")
    );

    let snapshot = snapshot_codebase(temp.path(), &extracted);
    insta::assert_snapshot!(
        "tree_sitter_backend_extracted_codebase",
        serde_json::to_string_pretty(&snapshot).unwrap()
    );
}

#[rstest]
#[case(
    "tree_sitter_backend_typescript_codebase",
    write_sample_typescript_project()
)]
#[case("tree_sitter_backend_python_codebase", write_sample_python_project())]
#[case("tree_sitter_backend_csharp_codebase", write_sample_csharp_project())]
fn tree_sitter_backend_snapshots_multilanguage_output(
    #[case] snapshot_name: &'static str,
    #[case] temp: TempDir,
) {
    let cfg = LithoConfig {
        extract_backend: ExtractBackend::TreeSitter,
        ..LithoConfig::default()
    };

    let extracted = extract_with_config(temp.path(), &cfg).unwrap();
    assert_eq!(extracted.statistics.total_files, 1);

    let snapshot = snapshot_codebase(temp.path(), &extracted);
    insta::assert_snapshot!(
        snapshot_name,
        serde_json::to_string_pretty(&snapshot).unwrap()
    );
}

#[test]
fn ast_grep_backend_gracefully_falls_back_when_binary_missing() {
    let temp = write_sample_rust_project();
    let cfg = LithoConfig {
        extract_backend: ExtractBackend::AstGrep,
        ast_grep_binary: Some("definitely-missing-sg-bin".to_string()),
        ..LithoConfig::default()
    };

    let extracted = extract_with_config(temp.path(), &cfg).unwrap();
    assert_eq!(extracted.statistics.total_files, 1);
    assert!(
        extracted.files[0]
            .interfaces
            .iter()
            .any(|iface| iface.name == "Settings")
    );
}

#[test]
fn ast_grep_backend_runs_when_sg_available() {
    let sg_available = Command::new("sg")
        .arg("--version")
        .status()
        .is_ok_and(|s| s.success());
    if !sg_available {
        return;
    }

    let temp = write_sample_rust_project();
    let cfg = LithoConfig {
        extract_backend: ExtractBackend::AstGrep,
        ast_grep_binary: Some("sg".to_string()),
        ..LithoConfig::default()
    };

    let extracted = extract_with_config(temp.path(), &cfg).unwrap();
    let file = &extracted.files[0];
    assert!(
        file.dependencies
            .iter()
            .any(|d| d.target == "std::path::PathBuf")
    );
}
