use assert_cmd::Command;
use serde_json::Value;
use std::fs;
use tempfile::TempDir;

fn write_project() -> TempDir {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("lib.rs"),
        r#"
use std::collections::HashMap;

pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
"#,
    )
    .unwrap();
    temp
}

fn run_extract(args: &[&str]) -> Value {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("litho"));
    let output = cmd.args(args).output().unwrap();
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn e2e_extract_json_with_tree_sitter_backend() {
    let project = write_project();
    let project_path = project.path().to_string_lossy().to_string();
    let output = run_extract(&[
        "extract",
        &project_path,
        "--format",
        "json",
        "--extract-backend",
        "tree-sitter",
    ]);

    assert_eq!(output["statistics"]["total_files"], 1);
    let interfaces = output["files"][0]["interfaces"].as_array().unwrap();
    assert!(interfaces.iter().any(|i| i["name"] == "add"));
}

#[test]
fn functional_extract_gracefully_handles_missing_ast_grep_binary() {
    let project = write_project();
    let project_path = project.path().to_string_lossy().to_string();
    let output = run_extract(&[
        "extract",
        &project_path,
        "--format",
        "json",
        "--extract-backend",
        "ast-grep",
        "--ast-grep-bin",
        "definitely-missing-sg-bin",
    ]);

    // Should still succeed through tree-sitter fallback.
    assert_eq!(output["statistics"]["total_files"], 1);
    let deps = output["files"][0]["dependencies"].as_array().unwrap();
    assert!(
        deps.iter()
            .any(|d| d["target"] == "std::collections::HashMap")
    );
}
