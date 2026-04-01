use assert_cmd::Command;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn write_minimal_project(root: &Path) {
    let src = root.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("lib.rs"),
        r#"
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
"#,
    )
    .unwrap();
}

fn run_litho(args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("litho"));
    cmd.args(args).output().unwrap()
}

fn as_string(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn status_no_manifest_prints_guidance() {
    let project = TempDir::new().unwrap();
    write_minimal_project(project.path());
    let project_path = project.path().to_string_lossy().to_string();

    let output = run_litho(&["status", &project_path]);
    assert!(
        output.status.success(),
        "command failed: {}",
        as_string(&output.stderr)
    );

    let stdout = as_string(&output.stdout);
    assert!(stdout.contains("No documentation manifest found"));
    assert!(stdout.contains("Run `litho-generator` to generate documentation first."));
}

#[test]
fn status_reads_manifest_and_counts() {
    let project = TempDir::new().unwrap();
    write_minimal_project(project.path());
    let internal = project.path().join(".litho");
    fs::create_dir_all(&internal).unwrap();

    let project_path_str = project.path().to_string_lossy().to_string();
    let manifest = r#"{
  "version": 1,
  "generated_at": "2026-03-05T12:00:00Z",
  "git_commit": "abc1234",
  "git_branch": "main",
  "file_hashes": {
    "src/lib.rs": "hash1",
    "src/main.rs": "hash2"
  },
  "modules": {
    "Overview": {}
  },
  "total_generation_time_secs": 12.5
}"#
    .to_string();
    fs::write(internal.join("manifest.json"), manifest).unwrap();

    let output = run_litho(&["status", &project_path_str]);
    assert!(
        output.status.success(),
        "command failed: {}",
        as_string(&output.stderr)
    );

    let stdout = as_string(&output.stdout);
    assert!(stdout.contains("Documentation Status"));
    assert!(stdout.contains("Manifest version : 1"));
    assert!(stdout.contains("Files tracked    : 2"));
    assert!(stdout.contains("Modules generated: 1"));
}

#[test]
fn serve_without_litho_book_prints_fallback_hint() {
    let project = TempDir::new().unwrap();
    let docs = project.path().join("docs");
    fs::create_dir_all(&docs).unwrap();
    let docs_path = docs.to_string_lossy().to_string();

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("litho"));
    let output = cmd
        .current_dir(project.path())
        .env("PATH", "")
        .args(["serve", &docs_path, "--port", "4444"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "command failed: {}",
        as_string(&output.stderr)
    );

    let stdout = as_string(&output.stdout);
    assert!(stdout.contains("litho-book not found in PATH."));
    assert!(stdout.contains("python -m http.server 4444 --directory"));
}

#[test]
fn validate_reports_broken_reference() {
    let project = TempDir::new().unwrap();
    let docs = project.path().join("docs");
    fs::create_dir_all(&docs).unwrap();
    fs::write(
        docs.join("overview.md"),
        "Broken reference to `src/missing.rs` should be detected.\n",
    )
    .unwrap();
    let docs_path = docs.to_string_lossy().to_string();

    let output = run_litho(&["validate", &docs_path]);
    assert!(
        output.status.success(),
        "command failed: {}",
        as_string(&output.stderr)
    );

    let stdout = as_string(&output.stdout);
    assert!(stdout.contains("Found 1 issue(s)"));
    assert!(stdout.contains("broken reference `src/missing.rs`"));
}

#[test]
fn validate_passes_when_references_exist() {
    let project = TempDir::new().unwrap();
    let docs = project.path().join("docs");
    let src = project.path().join("src");
    fs::create_dir_all(&docs).unwrap();
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("main.rs"), "fn main() {}\n").unwrap();
    fs::write(
        docs.join("overview.md"),
        "This file references `src/main.rs`.\n",
    )
    .unwrap();

    let docs_path = docs.to_string_lossy().to_string();
    let output = run_litho(&["validate", &docs_path]);
    assert!(
        output.status.success(),
        "command failed: {}",
        as_string(&output.stderr)
    );

    let stdout = as_string(&output.stdout);
    assert!(stdout.contains("No issues found"));
}

#[test]
fn validate_nested_docs_resolve_paths_against_repo_root() {
    let project = TempDir::new().unwrap();
    let docs = project.path().join("docs").join("auto").join("litho_docs");
    let src = project.path().join("src");
    fs::create_dir_all(&docs).unwrap();
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(project.path().join(".git")).unwrap();
    fs::write(src.join("main.rs"), "fn main() {}\n").unwrap();
    fs::write(
        docs.join("overview.md"),
        "This file references `src/main.rs`.\n",
    )
    .unwrap();

    let docs_path = docs.to_string_lossy().to_string();
    let output = run_litho(&["validate", &docs_path]);
    assert!(
        output.status.success(),
        "command failed: {}",
        as_string(&output.stderr)
    );

    let stdout = as_string(&output.stdout);
    assert!(stdout.contains("No issues found"));
}

#[test]
fn generate_missing_project_path_fails_fast() {
    let root = TempDir::new().unwrap();
    let missing_path = root.path().join("missing-project");
    let missing_path = missing_path.to_string_lossy().to_string();
    let out_dir = root.path().join("out");
    let out_dir = out_dir.to_string_lossy().to_string();

    let output = run_litho(&[
        "generate",
        &missing_path,
        "--provider",
        "codex-exec",
        "--output",
        &out_dir,
    ]);

    assert!(!output.status.success(), "generate unexpectedly succeeded");
    let stderr = as_string(&output.stderr);
    assert!(stderr.contains("project path does not exist"));
}

#[test]
fn generate_codex_exec_reports_readiness_failure_when_binary_missing() {
    let project = TempDir::new().unwrap();
    write_minimal_project(project.path());
    let output_dir = project.path().join("docs-out");
    let project_path = project.path().to_string_lossy().to_string();
    let output_path = output_dir.to_string_lossy().to_string();

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("litho"));
    let output = cmd
        .env("CODEX_BINARY_PATH", "definitely-missing-codex-binary")
        .args([
            "generate",
            &project_path,
            "--provider",
            "codex-exec",
            "--output",
            &output_path,
        ])
        .output()
        .unwrap();

    assert!(!output.status.success(), "generate unexpectedly succeeded");
    let stderr = as_string(&output.stderr);
    assert!(stderr.contains("documentation provider is not ready"));
}
