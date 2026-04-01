use litho_generator::benchmark::{BenchmarkOptimizationArgs, run_benchmark_optimization};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

fn dry_run_args(project_path: PathBuf, output_dir: PathBuf) -> BenchmarkOptimizationArgs {
    BenchmarkOptimizationArgs {
        config: None,
        project_path: Some(project_path),
        output_dir,
        models: Some("qwen2.5-coder:3b,gemma3:12b".to_string()),
        context_windows: Some("32768,131072".to_string()),
        num_predict: Some("512".to_string()),
        temperatures: None,
        top_p_values: None,
        top_k_values: None,
        repeat_penalty_values: None,
        max_in_flight_values: None,
        runs_per_candidate: 1,
        warmup_runs: 0,
        max_candidates: 6,
        run_timeout_seconds: 60,
        min_quality: 0.70,
        weight_quality: 0.60,
        weight_latency: 0.20,
        weight_throughput: 0.10,
        weight_memory: 0.10,
        weight_stability: 0.00,
        keep_cache: false,
        retain_artifacts: false,
        dry_run: true,
        gate_min_success_rate: None,
        gate_max_p95_seconds: None,
        gate_min_quality: None,
    }
}

fn make_project(temp_root: &Path) -> PathBuf {
    let project = temp_root.join("project");
    fs::create_dir_all(&project).expect("create project dir");
    fs::write(
        project.join("README.md"),
        "# benchmark regression fixture\n",
    )
    .expect("write project readme");
    project
}

fn collect_report_jsons(output_dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(output_dir)
        .expect("read benchmark output dir")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with("benchmark-report-") && name.ends_with(".json"))
                .unwrap_or(false)
        })
        .collect();
    files.sort();
    files
}

#[tokio::test]
async fn benchmark_report_schema_contains_key_metrics_fields() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = make_project(temp.path());
    let output_dir = temp.path().join("benchmark-out");
    let args = dry_run_args(project, output_dir);

    let report = run_benchmark_optimization(args)
        .await
        .expect("dry-run benchmark should succeed");

    assert!(report.dry_run, "report should indicate dry-run execution");
    assert!(
        !report.candidates.is_empty(),
        "expected at least one candidate profile in dry-run report"
    );
    assert!(
        report.candidates.len() <= 6,
        "expected candidate count to respect max_candidates"
    );

    for candidate in &report.candidates {
        assert_eq!(candidate.success_rate, 0.0);
        assert_eq!(candidate.p95_duration_seconds, 0.0);
        assert_eq!(candidate.avg_cold_duration_seconds, 0.0);
        assert_eq!(candidate.avg_incremental_duration_seconds, 0.0);
    }

    let raw = fs::read_to_string(&report.report_json_path).expect("read report json");
    let json: Value = serde_json::from_str(&raw).expect("parse report json");
    let candidates = json["candidates"]
        .as_array()
        .expect("report.candidates should be an array");
    assert!(
        !candidates.is_empty(),
        "expected at least one serialized candidate"
    );
    let first = &candidates[0];
    for key in [
        "success_rate",
        "p95_duration_seconds",
        "avg_cold_duration_seconds",
        "avg_incremental_duration_seconds",
        "composite_score",
    ] {
        assert!(
            first.get(key).is_some(),
            "expected serialized candidate field: {key}"
        );
    }
}

#[tokio::test]
async fn benchmark_report_candidate_order_is_stable_for_identical_dry_run_inputs() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = make_project(temp.path());
    let out_a = temp.path().join("benchmark-a");
    let out_b = temp.path().join("benchmark-b");

    let report_a = run_benchmark_optimization(dry_run_args(project.clone(), out_a))
        .await
        .expect("first dry-run benchmark should succeed");
    let report_b = run_benchmark_optimization(dry_run_args(project, out_b))
        .await
        .expect("second dry-run benchmark should succeed");

    let ids_a: Vec<&str> = report_a
        .candidates
        .iter()
        .map(|c| c.candidate.id.as_str())
        .collect();
    let ids_b: Vec<&str> = report_b
        .candidates
        .iter()
        .map(|c| c.candidate.id.as_str())
        .collect();

    assert_eq!(
        ids_a, ids_b,
        "candidate ordering should be deterministic across identical dry-run executions"
    );
}

#[tokio::test]
async fn benchmark_report_gate_failures_are_persisted_in_json_artifacts() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = make_project(temp.path());
    let output_dir = temp.path().join("benchmark-gated");
    let mut args = dry_run_args(project, output_dir.clone());
    args.gate_min_success_rate = Some(0.95);
    args.gate_min_quality = Some(0.80);
    args.gate_max_p95_seconds = Some(30.0);

    let err = run_benchmark_optimization(args)
        .await
        .expect_err("strict gates should fail in dry-run mode");
    assert!(
        err.to_string().contains("promotion gates failed"),
        "unexpected error text: {err}"
    );

    let report_files = collect_report_jsons(&output_dir);
    assert!(
        !report_files.is_empty(),
        "expected benchmark report json artifact after gate failure"
    );

    let raw = fs::read_to_string(&report_files[0]).expect("read failed-gate report json");
    let json: Value = serde_json::from_str(&raw).expect("parse failed-gate report json");
    let gate_failures = json["gate_failures"]
        .as_array()
        .expect("gate_failures must be an array");
    assert!(
        !gate_failures.is_empty(),
        "expected at least one serialized gate failure"
    );
}
