use litho_core::config::{ExtractBackend, LithoConfig};
use litho_extract::extract_with_config;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn write_sample_rust_project() -> TempDir {
    let temp = TempDir::new().unwrap();
    let src_dir = temp.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(
        src_dir.join("lib.rs"),
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
    .unwrap();
    temp
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
