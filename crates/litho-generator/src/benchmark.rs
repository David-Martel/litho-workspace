use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::{Config, LLMProvider};
use crate::generator::validator::ValidationReport;
use crate::generator::workflow::{launch, launch_incremental};

#[derive(Debug, Clone)]
pub struct BenchmarkOptimizationArgs {
    pub config: Option<PathBuf>,
    pub project_path: Option<PathBuf>,
    pub output_dir: PathBuf,
    pub models: Option<String>,
    pub context_windows: Option<String>,
    pub num_predict: Option<String>,
    pub temperatures: Option<String>,
    pub top_p_values: Option<String>,
    pub top_k_values: Option<String>,
    pub repeat_penalty_values: Option<String>,
    pub max_in_flight_values: Option<String>,
    pub runs_per_candidate: usize,
    pub warmup_runs: usize,
    pub max_candidates: usize,
    pub run_timeout_seconds: u64,
    pub min_quality: f64,
    pub weight_quality: f64,
    pub weight_latency: f64,
    pub weight_throughput: f64,
    pub weight_memory: f64,
    pub weight_stability: f64,
    pub keep_cache: bool,
    pub retain_artifacts: bool,
    pub dry_run: bool,
    pub gate_min_success_rate: Option<f64>,
    pub gate_max_p95_seconds: Option<f64>,
    pub gate_min_quality: Option<f64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct OptimizationWeights {
    pub quality: f64,
    pub latency: f64,
    pub throughput: f64,
    pub memory: f64,
    pub stability: f64,
}

impl OptimizationWeights {
    fn normalized(&self) -> Self {
        let sum = self.quality + self.latency + self.throughput + self.memory + self.stability;
        if sum <= f64::EPSILON {
            return Self::default();
        }
        Self {
            quality: self.quality / sum,
            latency: self.latency / sum,
            throughput: self.throughput / sum,
            memory: self.memory / sum,
            stability: self.stability / sum,
        }
    }
}

impl Default for OptimizationWeights {
    fn default() -> Self {
        Self {
            quality: 0.60,
            latency: 0.20,
            throughput: 0.10,
            memory: 0.10,
            stability: 0.00,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkOptimizationReport {
    pub generated_at: String,
    pub project_path: String,
    pub provider: String,
    pub dry_run: bool,
    pub run_timeout_seconds: u64,
    pub min_quality: f64,
    pub weights: OptimizationWeights,
    pub candidates: Vec<CandidateResult>,
    pub recommendation: Option<CandidateResult>,
    pub gate_failures: Vec<String>,
    pub report_json_path: String,
    pub report_markdown_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateProfile {
    pub id: String,
    pub profile: String,
    pub model: String,
    pub context_window: u32,
    pub num_predict: i32,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub top_k: Option<u32>,
    pub repeat_penalty: Option<f64>,
    pub max_in_flight: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSample {
    pub run_index: usize,
    pub mode: String,
    pub duration_seconds: f64,
    pub quality_score: Option<f64>,
    pub error_count: Option<usize>,
    pub warning_count: Option<usize>,
    pub doc_file_count: Option<usize>,
    pub doc_bytes: Option<u64>,
    pub succeeded: bool,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateResult {
    pub candidate: CandidateProfile,
    pub model_size_bytes: Option<u64>,
    pub model_size_gib: Option<f64>,
    pub run_samples: Vec<RunSample>,
    pub success_rate: f64,
    pub avg_quality_score: f64,
    pub avg_duration_seconds: f64,
    pub p95_duration_seconds: f64,
    pub avg_cold_duration_seconds: f64,
    pub avg_incremental_duration_seconds: f64,
    pub avg_doc_file_count: f64,
    pub avg_doc_bytes: f64,
    pub avg_throughput_bytes_per_second: f64,
    pub estimated_memory_index: f64,
    pub quality_gate_passed: bool,
    pub latency_norm: f64,
    pub throughput_norm: f64,
    pub memory_norm: f64,
    pub composite_score: f64,
}

pub async fn run_benchmark_optimization(
    args: BenchmarkOptimizationArgs,
) -> Result<BenchmarkOptimizationReport> {
    let mut base_config = load_base_config(args.config.as_ref())?;
    if let Some(project_path) = &args.project_path {
        base_config.project_path = project_path.clone();
    }
    if args.runs_per_candidate == 0 {
        anyhow::bail!("runs_per_candidate must be >= 1");
    }
    if args.max_candidates == 0 {
        anyhow::bail!("max_candidates must be >= 1");
    }
    if args.run_timeout_seconds == 0 {
        anyhow::bail!("run_timeout_seconds must be >= 1");
    }
    if let Some(v) = args.gate_min_success_rate
        && !(0.0..=1.0).contains(&v)
    {
        anyhow::bail!("gate_min_success_rate must be in [0, 1]");
    }
    if let Some(v) = args.gate_min_quality
        && !(0.0..=1.0).contains(&v)
    {
        anyhow::bail!("gate_min_quality must be in [0, 1]");
    }
    if let Some(v) = args.gate_max_p95_seconds
        && v <= 0.0
    {
        anyhow::bail!("gate_max_p95_seconds must be > 0");
    }

    let models = resolve_models(&args, &base_config)?;
    let mut candidates = build_candidates(&args, &base_config, &models)?;
    if candidates.len() > args.max_candidates {
        candidates.truncate(args.max_candidates);
    }

    let weights = OptimizationWeights {
        quality: args.weight_quality,
        latency: args.weight_latency,
        throughput: args.weight_throughput,
        memory: args.weight_memory,
        stability: args.weight_stability,
    }
    .normalized();

    fs::create_dir_all(&args.output_dir).with_context(|| {
        format!(
            "failed to create benchmark output directory: {}",
            args.output_dir.display()
        )
    })?;

    let model_sizes = fetch_model_sizes_if_available(&base_config).await;

    let mut results = Vec::with_capacity(candidates.len());
    if args.dry_run {
        for candidate in candidates.drain(..) {
            results.push(CandidateResult {
                model_size_bytes: select_model_size_bytes(&model_sizes, &candidate.model),
                model_size_gib: select_model_size_bytes(&model_sizes, &candidate.model)
                    .map(bytes_to_gib),
                estimated_memory_index: estimate_memory_index(
                    &candidate,
                    select_model_size_bytes(&model_sizes, &candidate.model),
                ),
                candidate,
                run_samples: Vec::new(),
                success_rate: 0.0,
                avg_quality_score: 0.0,
                avg_duration_seconds: 0.0,
                p95_duration_seconds: 0.0,
                avg_cold_duration_seconds: 0.0,
                avg_incremental_duration_seconds: 0.0,
                avg_doc_file_count: 0.0,
                avg_doc_bytes: 0.0,
                avg_throughput_bytes_per_second: 0.0,
                quality_gate_passed: false,
                latency_norm: 0.0,
                throughput_norm: 0.0,
                memory_norm: 0.0,
                composite_score: 0.0,
            });
        }
    } else {
        for (idx, candidate) in candidates.iter().enumerate() {
            println!(
                "[benchmark] candidate {}/{}: {}",
                idx + 1,
                candidates.len(),
                candidate.id
            );
            let result = run_candidate(
                &args,
                &base_config,
                candidate,
                &model_sizes,
                args.min_quality,
            )
            .await?;
            results.push(result);
        }
    }

    apply_scoring(&mut results, &weights, args.min_quality);
    results.sort_by(|a, b| {
        b.composite_score
            .partial_cmp(&a.composite_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let recommendation = select_recommendation(&results, args.min_quality).cloned();
    let gate_failures = evaluate_promotion_gates(&results, recommendation.as_ref(), &args);
    let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let json_path = args
        .output_dir
        .join(format!("benchmark-report-{timestamp}.json"));
    let markdown_path = args
        .output_dir
        .join(format!("benchmark-report-{timestamp}.md"));

    let report = BenchmarkOptimizationReport {
        generated_at: chrono::Utc::now().to_rfc3339(),
        project_path: base_config.project_path.display().to_string(),
        provider: format!("{:?}", base_config.llm.provider),
        dry_run: args.dry_run,
        run_timeout_seconds: args.run_timeout_seconds,
        min_quality: args.min_quality,
        weights,
        candidates: results,
        recommendation,
        gate_failures: gate_failures.clone(),
        report_json_path: json_path.display().to_string(),
        report_markdown_path: markdown_path.display().to_string(),
    };

    fs::write(
        &json_path,
        serde_json::to_string_pretty(&report).context("serialize benchmark report json")?,
    )
    .with_context(|| format!("failed to write report json: {}", json_path.display()))?;
    fs::write(&markdown_path, render_markdown_report(&report)).with_context(|| {
        format!(
            "failed to write report markdown: {}",
            markdown_path.display()
        )
    })?;

    println!(
        "[benchmark] wrote reports:\n  - {}\n  - {}",
        json_path.display(),
        markdown_path.display()
    );
    if let Some(best) = &report.recommendation {
        println!(
            "[benchmark] recommendation: {} (score {:.3}, quality {:.1}%, duration {:.1}s)",
            best.candidate.id,
            best.composite_score,
            best.avg_quality_score * 100.0,
            best.avg_duration_seconds
        );
    } else {
        println!("[benchmark] no passing recommendation found");
    }

    if !gate_failures.is_empty() {
        for failure in &gate_failures {
            eprintln!("[benchmark][gate] {failure}");
        }
        anyhow::bail!(
            "benchmark promotion gates failed ({} issue(s))",
            gate_failures.len()
        );
    }

    Ok(report)
}

async fn run_candidate(
    args: &BenchmarkOptimizationArgs,
    base_config: &Config,
    candidate: &CandidateProfile,
    model_sizes: &HashMap<String, u64>,
    min_quality: f64,
) -> Result<CandidateResult> {
    let model_size_bytes = select_model_size_bytes(model_sizes, &candidate.model);
    let mut samples = Vec::with_capacity(args.runs_per_candidate);
    let total_runs = args.warmup_runs + args.runs_per_candidate;
    let mut previous_run_succeeded = false;
    let candidate_state_root = args
        .output_dir
        .join("runs")
        .join(&candidate.id)
        .join("state");
    if !args.keep_cache && candidate_state_root.exists() {
        let _ = fs::remove_dir_all(&candidate_state_root);
    }

    for run_idx in 0..total_runs {
        let is_warmup = run_idx < args.warmup_runs;
        let run_slot = if is_warmup {
            format!("warmup-{}", run_idx + 1)
        } else {
            format!("run-{}", run_idx + 1 - args.warmup_runs)
        };
        let run_root = args
            .output_dir
            .join("runs")
            .join(&candidate.id)
            .join(run_slot.as_str());
        if run_root.exists() {
            let _ = fs::remove_dir_all(&run_root);
        }
        fs::create_dir_all(&run_root)
            .with_context(|| format!("failed to create run dir: {}", run_root.display()))?;

        let mut run_config = base_config.clone();
        run_config.output_path = run_root.join("docs");
        run_config.internal_path = if args.keep_cache {
            candidate_state_root.join(".litho")
        } else {
            run_root.join(".litho")
        };
        if !args.keep_cache {
            run_config.cache.enabled = false;
        }
        apply_candidate_to_config(&mut run_config, candidate);

        let started = Instant::now();
        let is_incremental_mode = args.keep_cache && run_idx > 0 && previous_run_succeeded;
        let result = if is_incremental_mode {
            tokio::time::timeout(
                Duration::from_secs(args.run_timeout_seconds),
                launch_incremental(&run_config),
            )
            .await
        } else {
            tokio::time::timeout(
                Duration::from_secs(args.run_timeout_seconds),
                launch(&run_config),
            )
            .await
        };
        let elapsed = started.elapsed().as_secs_f64();
        let mode = if is_incremental_mode {
            "incremental".to_string()
        } else {
            "cold".to_string()
        };

        let sample = match result {
            Ok(Ok(())) => {
                let validation_path = run_config.output_path.join("validation-report.json");
                let report = read_validation_report(&validation_path).ok();
                let (doc_file_count, doc_bytes) = collect_doc_stats(&run_config.output_path);
                RunSample {
                    run_index: run_idx + 1,
                    mode: mode.clone(),
                    duration_seconds: elapsed,
                    quality_score: report.as_ref().map(|r| r.quality_score),
                    error_count: report.as_ref().map(ValidationReport::errors),
                    warning_count: report.as_ref().map(ValidationReport::warnings),
                    doc_file_count: Some(doc_file_count),
                    doc_bytes: Some(doc_bytes),
                    succeeded: true,
                    error_message: None,
                }
            }
            Ok(Err(err)) => RunSample {
                run_index: run_idx + 1,
                mode: mode.clone(),
                duration_seconds: elapsed,
                quality_score: None,
                error_count: None,
                warning_count: None,
                doc_file_count: None,
                doc_bytes: None,
                succeeded: false,
                error_message: Some(err.to_string()),
            },
            Err(_) => RunSample {
                run_index: run_idx + 1,
                mode,
                duration_seconds: elapsed,
                quality_score: None,
                error_count: None,
                warning_count: None,
                doc_file_count: None,
                doc_bytes: None,
                succeeded: false,
                error_message: Some(format!(
                    "benchmark candidate run exceeded timeout of {}s",
                    args.run_timeout_seconds
                )),
            },
        };

        let run_succeeded = sample.succeeded;
        if !is_warmup {
            samples.push(sample);
        }
        previous_run_succeeded = run_succeeded;

        if !args.retain_artifacts {
            let _ = fs::remove_dir_all(&run_root);
        }
    }

    let success_samples: Vec<&RunSample> = samples.iter().filter(|s| s.succeeded).collect();
    let success_rate = if samples.is_empty() {
        0.0
    } else {
        success_samples.len() as f64 / samples.len() as f64
    };

    let avg_duration_seconds = mean(success_samples.iter().map(|s| s.duration_seconds));
    let p95_duration_seconds = percentile(samples.iter().map(|s| s.duration_seconds), 0.95);
    let avg_cold_duration_seconds = mean(
        samples
            .iter()
            .filter(|s| s.mode == "cold")
            .map(|s| s.duration_seconds),
    );
    let avg_incremental_duration_seconds = mean(
        samples
            .iter()
            .filter(|s| s.mode == "incremental")
            .map(|s| s.duration_seconds),
    );
    let avg_quality_score = mean(success_samples.iter().filter_map(|s| s.quality_score));
    let avg_doc_file_count = mean(
        success_samples
            .iter()
            .filter_map(|s| s.doc_file_count)
            .map(|v| v as f64),
    );
    let avg_doc_bytes = mean(
        success_samples
            .iter()
            .filter_map(|s| s.doc_bytes)
            .map(|v| v as f64),
    );
    let avg_throughput_bytes_per_second = if avg_duration_seconds > 0.0 {
        avg_doc_bytes / avg_duration_seconds
    } else {
        0.0
    };

    Ok(CandidateResult {
        candidate: candidate.clone(),
        model_size_bytes,
        model_size_gib: model_size_bytes.map(bytes_to_gib),
        run_samples: samples,
        success_rate,
        avg_quality_score,
        avg_duration_seconds,
        p95_duration_seconds,
        avg_cold_duration_seconds,
        avg_incremental_duration_seconds,
        avg_doc_file_count,
        avg_doc_bytes,
        avg_throughput_bytes_per_second,
        estimated_memory_index: estimate_memory_index(candidate, model_size_bytes),
        quality_gate_passed: avg_quality_score >= min_quality && success_rate > 0.0,
        latency_norm: 0.0,
        throughput_norm: 0.0,
        memory_norm: 0.0,
        composite_score: 0.0,
    })
}

fn apply_scoring(results: &mut [CandidateResult], weights: &OptimizationWeights, min_quality: f64) {
    if results.is_empty() {
        return;
    }
    let latency_minmax = minmax(results.iter().map(|r| r.avg_duration_seconds));
    let throughput_minmax = minmax(results.iter().map(|r| r.avg_throughput_bytes_per_second));
    let memory_minmax = minmax(results.iter().map(|r| r.estimated_memory_index));

    for result in results {
        result.quality_gate_passed =
            result.avg_quality_score >= min_quality && result.success_rate > 0.0;
        result.latency_norm = normalize_low(
            result.avg_duration_seconds,
            latency_minmax.0,
            latency_minmax.1,
        );
        result.throughput_norm = normalize_high(
            result.avg_throughput_bytes_per_second,
            throughput_minmax.0,
            throughput_minmax.1,
        );
        result.memory_norm = normalize_low(
            result.estimated_memory_index,
            memory_minmax.0,
            memory_minmax.1,
        );

        let mut score = weights.quality * result.avg_quality_score
            + weights.latency * result.latency_norm
            + weights.throughput * result.throughput_norm
            + weights.memory * result.memory_norm
            + weights.stability * result.success_rate;

        if !result.quality_gate_passed {
            score *= 0.5;
        }
        if result.success_rate <= 0.0 {
            score = 0.0;
        }
        result.composite_score = score;
    }
}

fn select_recommendation(
    results: &[CandidateResult],
    min_quality: f64,
) -> Option<&CandidateResult> {
    if let Some(passing) = results
        .iter()
        .filter(|r| r.success_rate > 0.0 && r.avg_quality_score >= min_quality)
        .max_by(|a, b| {
            a.composite_score
                .partial_cmp(&b.composite_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    {
        return Some(passing);
    }

    results
        .iter()
        .filter(|r| r.success_rate > 0.0)
        .max_by(|a, b| {
            a.composite_score
                .partial_cmp(&b.composite_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

fn evaluate_promotion_gates(
    results: &[CandidateResult],
    recommendation: Option<&CandidateResult>,
    args: &BenchmarkOptimizationArgs,
) -> Vec<String> {
    let mut failures = Vec::new();
    if args.gate_min_success_rate.is_none()
        && args.gate_max_p95_seconds.is_none()
        && args.gate_min_quality.is_none()
    {
        return failures;
    }
    let Some(candidate) = recommendation.or_else(|| results.first()) else {
        failures.push("no candidate results were produced".to_string());
        return failures;
    };

    if let Some(min_success) = args.gate_min_success_rate
        && candidate.success_rate < min_success
    {
        failures.push(format!(
            "candidate '{}' success_rate {:.3} < required {:.3}",
            candidate.candidate.id, candidate.success_rate, min_success
        ));
    }
    if let Some(min_quality) = args.gate_min_quality
        && candidate.avg_quality_score < min_quality
    {
        failures.push(format!(
            "candidate '{}' quality {:.3} < required {:.3}",
            candidate.candidate.id, candidate.avg_quality_score, min_quality
        ));
    }
    if let Some(max_p95) = args.gate_max_p95_seconds
        && candidate.p95_duration_seconds > max_p95
    {
        failures.push(format!(
            "candidate '{}' p95 {:.2}s > allowed {:.2}s",
            candidate.candidate.id, candidate.p95_duration_seconds, max_p95
        ));
    }
    failures
}

fn resolve_models(args: &BenchmarkOptimizationArgs, base: &Config) -> Result<Vec<String>> {
    let mut models = Vec::new();
    if let Some(raw) = args.models.as_deref() {
        models.extend(parse_csv_strings(raw));
    } else {
        if !base.llm.model_efficient.trim().is_empty() {
            models.push(base.llm.model_efficient.clone());
        }
        if !base.llm.model_powerful.trim().is_empty() {
            models.push(base.llm.model_powerful.clone());
        }
    }

    models.sort();
    models.dedup();
    if models.is_empty() {
        anyhow::bail!(
            "no benchmark models provided; use --models or set model_efficient/model_powerful in config"
        );
    }
    Ok(models)
}

fn build_candidates(
    args: &BenchmarkOptimizationArgs,
    base: &Config,
    models: &[String],
) -> Result<Vec<CandidateProfile>> {
    let context_windows =
        parse_csv_numbers::<u32>(args.context_windows.as_deref(), "context_windows")?;
    let num_predict = parse_csv_numbers::<i32>(args.num_predict.as_deref(), "num_predict")?;
    let temperatures = parse_csv_numbers::<f64>(args.temperatures.as_deref(), "temperatures")?;
    let top_p_values = parse_csv_numbers::<f64>(args.top_p_values.as_deref(), "top_p_values")?;
    let top_k_values = parse_csv_numbers::<u32>(args.top_k_values.as_deref(), "top_k_values")?;
    let repeat_penalty_values = parse_csv_numbers::<f64>(
        args.repeat_penalty_values.as_deref(),
        "repeat_penalty_values",
    )?;
    let max_in_flight_values =
        parse_csv_numbers::<usize>(args.max_in_flight_values.as_deref(), "max_in_flight_values")?;

    let has_grid_override = context_windows.is_some()
        || num_predict.is_some()
        || temperatures.is_some()
        || top_p_values.is_some()
        || top_k_values.is_some()
        || repeat_penalty_values.is_some()
        || max_in_flight_values.is_some();

    if has_grid_override {
        let contexts = context_windows.unwrap_or_else(|| vec![base.llm.context_window.max(1024)]);
        let predicts = num_predict.unwrap_or_else(|| {
            vec![
                base.llm
                    .ollama_num_predict
                    .unwrap_or(base.llm.max_tokens as i32),
            ]
        });
        let temps = temperatures.unwrap_or_else(|| vec![base.llm.temperature.unwrap_or(0.1)]);
        let top_ps = top_p_values.unwrap_or_else(|| vec![base.llm.ollama_top_p.unwrap_or(0.9)]);
        let top_ks = top_k_values.unwrap_or_else(|| vec![base.llm.ollama_top_k.unwrap_or(40)]);
        let penalties = repeat_penalty_values
            .unwrap_or_else(|| vec![base.llm.ollama_repeat_penalty.unwrap_or(1.1)]);
        let in_flights = max_in_flight_values.unwrap_or_else(|| {
            vec![
                base.llm
                    .ollama_max_in_flight
                    .unwrap_or(base.llm.max_parallels)
                    .max(1),
            ]
        });

        let mut out = Vec::new();
        for model in models {
            for context in &contexts {
                for predict in &predicts {
                    for temp in &temps {
                        for top_p in &top_ps {
                            for top_k in &top_ks {
                                for penalty in &penalties {
                                    for in_flight in &in_flights {
                                        let id = format!(
                                            "{}-ctx{}-np{}-tp{}-tk{}-rp{}-if{}",
                                            sanitize_id(model),
                                            context,
                                            predict,
                                            (*top_p * 100.0).round() as i64,
                                            top_k,
                                            (*penalty * 100.0).round() as i64,
                                            in_flight
                                        );
                                        out.push(CandidateProfile {
                                            id,
                                            profile: "grid".to_string(),
                                            model: model.clone(),
                                            context_window: (*context).max(1024),
                                            num_predict: (*predict).max(64),
                                            temperature: Some(*temp),
                                            top_p: Some(*top_p),
                                            top_k: Some(*top_k),
                                            repeat_penalty: Some(*penalty),
                                            max_in_flight: (*in_flight).max(1),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        return Ok(out);
    }

    let mut out = Vec::new();
    for model in models {
        let model_default_ctx =
            crate::config::model_default_context_window(&model.to_ascii_lowercase());
        let cap = if base.llm.context_window > 0 {
            base.llm.context_window.min(model_default_ctx)
        } else {
            model_default_ctx
        };
        let fast_ctx = cap.clamp(4096, 8192);
        let balanced_ctx = cap.clamp(8192, 32768);
        let quality_ctx = cap.clamp(16384, 65536);

        let fast_np = (base.llm.max_tokens as i32).clamp(512, 1024);
        let balanced_np = (base.llm.max_tokens as i32).clamp(1024, 3072);
        let quality_np = (base.llm.max_tokens as i32).clamp(2048, 4096);

        out.push(CandidateProfile {
            id: format!("{}-fast", sanitize_id(model)),
            profile: "fast".to_string(),
            model: model.clone(),
            context_window: fast_ctx,
            num_predict: fast_np,
            temperature: Some(base.llm.temperature.unwrap_or(0.1).min(0.2)),
            top_p: Some(0.85),
            top_k: Some(20),
            repeat_penalty: Some(1.05),
            max_in_flight: base
                .llm
                .ollama_max_in_flight
                .unwrap_or(base.llm.max_parallels)
                .clamp(1, 2),
        });
        out.push(CandidateProfile {
            id: format!("{}-balanced", sanitize_id(model)),
            profile: "balanced".to_string(),
            model: model.clone(),
            context_window: balanced_ctx,
            num_predict: balanced_np,
            temperature: Some(base.llm.temperature.unwrap_or(0.1)),
            top_p: Some(0.90),
            top_k: Some(40),
            repeat_penalty: Some(1.10),
            max_in_flight: base
                .llm
                .ollama_max_in_flight
                .unwrap_or(base.llm.max_parallels)
                .clamp(1, 3),
        });
        out.push(CandidateProfile {
            id: format!("{}-quality", sanitize_id(model)),
            profile: "quality".to_string(),
            model: model.clone(),
            context_window: quality_ctx,
            num_predict: quality_np,
            temperature: Some(base.llm.temperature.unwrap_or(0.1).max(0.1)),
            top_p: Some(0.95),
            top_k: Some(80),
            repeat_penalty: Some(1.15),
            max_in_flight: 1,
        });
    }

    Ok(out)
}

fn apply_candidate_to_config(config: &mut Config, candidate: &CandidateProfile) {
    config.llm.provider = LLMProvider::Ollama;
    config.llm.model_efficient = candidate.model.clone();
    config.llm.model_powerful = candidate.model.clone();
    config.llm.context_window = candidate.context_window;
    config.llm.ollama_adaptive_context_min = candidate.context_window.min(4096);
    config.llm.ollama_adaptive_context_max = candidate.context_window;
    config.llm.ollama_num_predict = Some(candidate.num_predict);
    config.llm.temperature = candidate.temperature;
    config.llm.ollama_top_p = candidate.top_p;
    config.llm.ollama_top_k = candidate.top_k;
    config.llm.ollama_repeat_penalty = candidate.repeat_penalty;
    config.llm.ollama_repeat_last_n = Some(128);
    config.llm.ollama_tfs_z = Some(1.0);
    config.llm.ollama_max_in_flight = Some(candidate.max_in_flight.max(1));
    config.llm.ollama_log_perf_metrics = true;
}

async fn fetch_model_sizes_if_available(config: &Config) -> HashMap<String, u64> {
    if config.llm.provider != LLMProvider::Ollama || config.llm.api_base_url.trim().is_empty() {
        return HashMap::new();
    }

    #[derive(Debug, Deserialize)]
    struct TagsResponse {
        #[serde(default)]
        models: Vec<TagModel>,
    }
    #[derive(Debug, Deserialize)]
    struct TagModel {
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        size: Option<u64>,
    }

    let endpoint = format!("{}/api/tags", config.llm.api_base_url.trim_end_matches('/'));
    let resp = match reqwest::Client::new().get(endpoint).send().await {
        Ok(v) => v,
        Err(_) => return HashMap::new(),
    };
    if !resp.status().is_success() {
        return HashMap::new();
    }
    let body: TagsResponse = match resp.json().await {
        Ok(v) => v,
        Err(_) => return HashMap::new(),
    };
    let mut out = HashMap::new();
    for model in body.models {
        let size = match model.size {
            Some(v) if v > 0 => v,
            _ => continue,
        };
        if let Some(name) = model.name {
            out.insert(name.clone(), size);
            let base = name.split(':').next().unwrap_or(name.as_str()).to_string();
            out.entry(base).or_insert(size);
        }
        if let Some(name) = model.model {
            out.insert(name.clone(), size);
            let base = name.split(':').next().unwrap_or(name.as_str()).to_string();
            out.entry(base).or_insert(size);
        }
    }
    out
}

fn select_model_size_bytes(model_sizes: &HashMap<String, u64>, model: &str) -> Option<u64> {
    if let Some(v) = model_sizes.get(model) {
        return Some(*v);
    }
    let base = model.split(':').next().unwrap_or(model);
    model_sizes.get(base).copied()
}

fn read_validation_report(path: &Path) -> Result<ValidationReport> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read validation report: {}", path.display()))?;
    serde_json::from_str::<ValidationReport>(&raw)
        .with_context(|| format!("failed to parse validation report: {}", path.display()))
}

fn collect_doc_stats(root: &Path) -> (usize, u64) {
    if !root.exists() {
        return (0, 0);
    }

    let mut files = 0usize;
    let mut bytes = 0u64;
    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let ext = entry
            .path()
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("html") {
            files += 1;
            if let Ok(meta) = entry.metadata() {
                bytes = bytes.saturating_add(meta.len());
            }
        }
    }

    (files, bytes)
}

fn estimate_memory_index(candidate: &CandidateProfile, model_size_bytes: Option<u64>) -> f64 {
    let model_term = model_size_bytes.map(bytes_to_gib).unwrap_or(1.0).max(0.5);
    let ctx_term = candidate.context_window as f64 / 32768.0;
    let predict_term = candidate.num_predict.max(1) as f64 / 2048.0;
    let inflight_term = candidate.max_in_flight.max(1) as f64;
    model_term * inflight_term * (1.0 + 0.35 * ctx_term + 0.15 * predict_term)
}

fn load_base_config(path: Option<&PathBuf>) -> Result<Config> {
    if let Some(path) = path {
        return Config::from_file(path)
            .with_context(|| format!("failed to load config from {}", path.display()));
    }
    let default = PathBuf::from("litho.toml");
    if default.exists() {
        return Config::from_file(&default).context("failed to load config from litho.toml");
    }
    Ok(Config::default())
}

fn parse_csv_strings(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect()
}

fn parse_csv_numbers<T>(raw: Option<&str>, field_name: &str) -> Result<Option<Vec<T>>>
where
    T: std::str::FromStr,
    <T as std::str::FromStr>::Err: std::fmt::Display,
{
    let Some(raw) = raw else {
        return Ok(None);
    };
    let mut out = Vec::new();
    for token in raw.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
        let value = token.parse::<T>().map_err(|err| {
            anyhow::anyhow!("failed to parse value '{token}' in field '{field_name}': {err}")
        })?;
        out.push(value);
    }
    if out.is_empty() {
        Ok(None)
    } else {
        Ok(Some(out))
    }
}

fn sanitize_id(model: &str) -> String {
    model
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn mean<I>(iter: I) -> f64
where
    I: IntoIterator<Item = f64>,
{
    let mut sum = 0.0;
    let mut count = 0usize;
    for value in iter {
        sum += value;
        count += 1;
    }
    if count == 0 { 0.0 } else { sum / count as f64 }
}

fn percentile<I>(iter: I, quantile: f64) -> f64
where
    I: IntoIterator<Item = f64>,
{
    let mut values: Vec<f64> = iter.into_iter().filter(|v| v.is_finite()).collect();
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let q = quantile.clamp(0.0, 1.0);
    let idx = ((values.len() - 1) as f64 * q).round() as usize;
    values[idx.min(values.len() - 1)]
}

fn minmax<I>(iter: I) -> (f64, f64)
where
    I: IntoIterator<Item = f64>,
{
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for value in iter {
        if !value.is_finite() {
            continue;
        }
        min = min.min(value);
        max = max.max(value);
    }
    if !min.is_finite() || !max.is_finite() {
        (0.0, 0.0)
    } else {
        (min, max)
    }
}

fn normalize_high(value: f64, min: f64, max: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    if (max - min).abs() <= f64::EPSILON {
        return 1.0;
    }
    ((value - min) / (max - min)).clamp(0.0, 1.0)
}

fn normalize_low(value: f64, min: f64, max: f64) -> f64 {
    1.0 - normalize_high(value, min, max)
}

fn bytes_to_gib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0 * 1024.0)
}

fn render_markdown_report(report: &BenchmarkOptimizationReport) -> String {
    let mut out = String::new();
    out.push_str("# Litho Benchmark Optimization Report\n\n");
    out.push_str(&format!("- Generated at: `{}`\n", report.generated_at));
    out.push_str(&format!("- Project path: `{}`\n", report.project_path));
    out.push_str(&format!("- Provider: `{}`\n", report.provider));
    out.push_str(&format!("- Dry run: `{}`\n", report.dry_run));
    out.push_str(&format!(
        "- Per-run timeout: `{}s`\n",
        report.run_timeout_seconds
    ));
    out.push_str(&format!(
        "- Min quality: `{:.1}%`\n",
        report.min_quality * 100.0
    ));
    out.push_str("\n## Weights\n\n");
    out.push_str(&format!(
        "- quality: `{:.2}` latency: `{:.2}` throughput: `{:.2}` memory: `{:.2}` stability: `{:.2}`\n",
        report.weights.quality,
        report.weights.latency,
        report.weights.throughput,
        report.weights.memory,
        report.weights.stability
    ));
    if !report.gate_failures.is_empty() {
        out.push_str("\n## Promotion Gates\n\n");
        out.push_str("- Status: `FAILED`\n");
        for failure in &report.gate_failures {
            out.push_str(&format!("- {}\n", failure));
        }
    }

    out.push_str("\n## Recommendation\n\n");
    if let Some(best) = &report.recommendation {
        out.push_str(&format!(
            "- `{}` (`{}`) score `{:.3}`, quality `{:.1}%`, duration `{:.1}s`, memory-index `{:.2}`\n",
            best.candidate.id,
            best.candidate.model,
            best.composite_score,
            best.avg_quality_score * 100.0,
            best.avg_duration_seconds,
            best.estimated_memory_index
        ));
    } else {
        out.push_str("- No recommendation available.\n");
    }

    out.push_str("\n## Candidates\n\n");
    out.push_str(
        "| Rank | Candidate | Model | Quality | Avg(s) | P95(s) | Cold(s) | Incr(s) | Throughput(B/s) | MemoryIdx | Success | Score |\n",
    );
    out.push_str(
        "| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n",
    );
    for (idx, row) in report.candidates.iter().enumerate() {
        out.push_str(&format!(
            "| {} | `{}` | `{}` | {:.1}% | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} | {:.2} | {:.0}% | {:.3} |\n",
            idx + 1,
            row.candidate.id,
            row.candidate.model,
            row.avg_quality_score * 100.0,
            row.avg_duration_seconds,
            row.p95_duration_seconds,
            row.avg_cold_duration_seconds,
            row.avg_incremental_duration_seconds,
            row.avg_throughput_bytes_per_second,
            row.estimated_memory_index,
            row.success_rate * 100.0,
            row.composite_score
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_normalize_to_one() {
        let w = OptimizationWeights {
            quality: 3.0,
            latency: 1.0,
            throughput: 1.0,
            memory: 0.0,
            stability: 0.0,
        }
        .normalized();
        let sum = w.quality + w.latency + w.throughput + w.memory + w.stability;
        assert!((sum - 1.0).abs() < 1e-9);
    }

    #[test]
    fn parse_csv_numbers_invalid_fails() {
        let parsed = parse_csv_numbers::<u32>(Some("1,2,abc"), "x");
        assert!(parsed.is_err());
    }

    #[test]
    fn heuristic_candidates_build_three_profiles_per_model() {
        let args = BenchmarkOptimizationArgs {
            config: None,
            project_path: None,
            output_dir: PathBuf::from("."),
            models: None,
            context_windows: None,
            num_predict: None,
            temperatures: None,
            top_p_values: None,
            top_k_values: None,
            repeat_penalty_values: None,
            max_in_flight_values: None,
            runs_per_candidate: 1,
            warmup_runs: 0,
            max_candidates: 24,
            run_timeout_seconds: 300,
            min_quality: 0.7,
            gate_min_success_rate: None,
            gate_max_p95_seconds: None,
            gate_min_quality: None,
            weight_quality: 0.6,
            weight_latency: 0.2,
            weight_throughput: 0.1,
            weight_memory: 0.1,
            weight_stability: 0.0,
            keep_cache: false,
            retain_artifacts: false,
            dry_run: true,
        };
        let mut config = Config::default();
        config.llm.model_efficient = "gemma3:4b".to_string();
        config.llm.model_powerful = "qwen2.5-coder:7b".to_string();
        let models = resolve_models(&args, &config).unwrap();
        let candidates = build_candidates(&args, &config, &models).unwrap();
        assert_eq!(candidates.len(), 6);
    }

    #[test]
    fn scoring_prefers_higher_quality_when_other_metrics_close() {
        let candidate = CandidateProfile {
            id: "a".to_string(),
            profile: "balanced".to_string(),
            model: "model-a".to_string(),
            context_window: 32768,
            num_predict: 2048,
            temperature: Some(0.1),
            top_p: Some(0.9),
            top_k: Some(40),
            repeat_penalty: Some(1.1),
            max_in_flight: 1,
        };
        let mut results = vec![
            CandidateResult {
                candidate: candidate.clone(),
                model_size_bytes: None,
                model_size_gib: None,
                run_samples: Vec::new(),
                success_rate: 1.0,
                avg_quality_score: 0.90,
                avg_duration_seconds: 12.0,
                p95_duration_seconds: 12.0,
                avg_cold_duration_seconds: 12.0,
                avg_incremental_duration_seconds: 0.0,
                avg_doc_file_count: 10.0,
                avg_doc_bytes: 2000.0,
                avg_throughput_bytes_per_second: 200.0,
                estimated_memory_index: 1.0,
                quality_gate_passed: true,
                latency_norm: 0.0,
                throughput_norm: 0.0,
                memory_norm: 0.0,
                composite_score: 0.0,
            },
            CandidateResult {
                candidate: CandidateProfile {
                    id: "b".to_string(),
                    ..candidate
                },
                model_size_bytes: None,
                model_size_gib: None,
                run_samples: Vec::new(),
                success_rate: 1.0,
                avg_quality_score: 0.70,
                avg_duration_seconds: 10.0,
                p95_duration_seconds: 10.0,
                avg_cold_duration_seconds: 10.0,
                avg_incremental_duration_seconds: 0.0,
                avg_doc_file_count: 10.0,
                avg_doc_bytes: 2100.0,
                avg_throughput_bytes_per_second: 210.0,
                estimated_memory_index: 1.0,
                quality_gate_passed: true,
                latency_norm: 0.0,
                throughput_norm: 0.0,
                memory_norm: 0.0,
                composite_score: 0.0,
            },
        ];
        apply_scoring(
            &mut results,
            &OptimizationWeights {
                quality: 0.95,
                latency: 0.03,
                throughput: 0.02,
                memory: 0.0,
                stability: 0.0,
            },
            0.6,
        );
        results.sort_by(|a, b| {
            b.composite_score
                .partial_cmp(&a.composite_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        assert_eq!(results[0].candidate.id, "a");
    }

    #[test]
    fn promotion_gates_fail_when_candidate_violates_thresholds() {
        let candidate = CandidateResult {
            candidate: CandidateProfile {
                id: "cand-a".to_string(),
                profile: "balanced".to_string(),
                model: "qwen2.5-coder:3b".to_string(),
                context_window: 32768,
                num_predict: 2048,
                temperature: Some(0.1),
                top_p: Some(0.9),
                top_k: Some(40),
                repeat_penalty: Some(1.1),
                max_in_flight: 1,
            },
            model_size_bytes: None,
            model_size_gib: None,
            run_samples: Vec::new(),
            success_rate: 0.4,
            avg_quality_score: 0.6,
            avg_duration_seconds: 30.0,
            p95_duration_seconds: 45.0,
            avg_cold_duration_seconds: 30.0,
            avg_incremental_duration_seconds: 0.0,
            avg_doc_file_count: 1.0,
            avg_doc_bytes: 1024.0,
            avg_throughput_bytes_per_second: 30.0,
            estimated_memory_index: 1.0,
            quality_gate_passed: false,
            latency_norm: 0.0,
            throughput_norm: 0.0,
            memory_norm: 0.0,
            composite_score: 0.0,
        };
        let args = BenchmarkOptimizationArgs {
            config: None,
            project_path: None,
            output_dir: PathBuf::from("."),
            models: None,
            context_windows: None,
            num_predict: None,
            temperatures: None,
            top_p_values: None,
            top_k_values: None,
            repeat_penalty_values: None,
            max_in_flight_values: None,
            runs_per_candidate: 1,
            warmup_runs: 0,
            max_candidates: 1,
            run_timeout_seconds: 60,
            min_quality: 0.7,
            gate_min_success_rate: Some(0.8),
            gate_max_p95_seconds: Some(20.0),
            gate_min_quality: Some(0.8),
            weight_quality: 0.6,
            weight_latency: 0.2,
            weight_throughput: 0.1,
            weight_memory: 0.1,
            weight_stability: 0.0,
            keep_cache: false,
            retain_artifacts: false,
            dry_run: true,
        };
        let failures =
            evaluate_promotion_gates(std::slice::from_ref(&candidate), Some(&candidate), &args);
        assert_eq!(failures.len(), 3);
    }

    fn make_benchmark_args(
        config: PathBuf,
        project_path: PathBuf,
        output_dir: PathBuf,
    ) -> BenchmarkOptimizationArgs {
        BenchmarkOptimizationArgs {
            config: Some(config),
            project_path: Some(project_path),
            output_dir,
            models: None,
            context_windows: None,
            num_predict: None,
            temperatures: None,
            top_p_values: None,
            top_k_values: None,
            repeat_penalty_values: None,
            max_in_flight_values: None,
            runs_per_candidate: 1,
            warmup_runs: 0,
            max_candidates: 8,
            run_timeout_seconds: 60,
            min_quality: 0.7,
            gate_min_success_rate: None,
            gate_max_p95_seconds: None,
            gate_min_quality: None,
            weight_quality: 0.6,
            weight_latency: 0.2,
            weight_throughput: 0.1,
            weight_memory: 0.1,
            weight_stability: 0.0,
            keep_cache: false,
            retain_artifacts: false,
            dry_run: true,
        }
    }

    #[tokio::test]
    async fn benchmark_dry_run_writes_reports() {
        let temp = tempfile::tempdir().expect("tempdir");
        let project = temp.path().join("project");
        std::fs::create_dir_all(&project).expect("create project");
        std::fs::write(project.join("README.md"), "# Bench Project\n").expect("write readme");

        let config_path = temp.path().join("litho.toml");
        let config_toml = format!(
            "project_path = \"{}\"\n\n[llm]\nprovider = \"ollama\"\nmodel_efficient = \"qwen2.5-coder:3b\"\nmodel_powerful = \"gemma3:12b\"\n",
            project.display().to_string().replace('\\', "/")
        );
        std::fs::write(&config_path, config_toml).expect("write config");

        let output_dir = temp.path().join("benchmark-output");
        let args = make_benchmark_args(config_path, project, output_dir.clone());
        let report = run_benchmark_optimization(args)
            .await
            .expect("dry run should succeed");

        assert!(report.dry_run);
        assert!(!report.candidates.is_empty());
        assert!(Path::new(&report.report_json_path).exists());
        assert!(Path::new(&report.report_markdown_path).exists());
    }

    #[tokio::test]
    async fn benchmark_dry_run_with_gates_returns_error_and_keeps_artifacts() {
        let temp = tempfile::tempdir().expect("tempdir");
        let project = temp.path().join("project");
        std::fs::create_dir_all(&project).expect("create project");
        std::fs::write(project.join("README.md"), "# Bench Project\n").expect("write readme");

        let config_path = temp.path().join("litho.toml");
        let config_toml = format!(
            "project_path = \"{}\"\n\n[llm]\nprovider = \"ollama\"\nmodel_efficient = \"qwen2.5-coder:3b\"\nmodel_powerful = \"gemma3:12b\"\n",
            project.display().to_string().replace('\\', "/")
        );
        std::fs::write(&config_path, config_toml).expect("write config");

        let output_dir = temp.path().join("benchmark-output");
        let mut args = make_benchmark_args(config_path, project, output_dir.clone());
        args.gate_min_success_rate = Some(0.8);
        args.gate_min_quality = Some(0.8);
        args.gate_max_p95_seconds = Some(30.0);

        let err = run_benchmark_optimization(args)
            .await
            .expect_err("dry run with strict gates should fail");
        assert!(
            err.to_string().contains("promotion gates failed"),
            "unexpected error: {err}"
        );

        let entries = std::fs::read_dir(&output_dir).expect("read output dir");
        let json_reports = entries
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                let name = entry.file_name().to_string_lossy().to_string();
                name.starts_with("benchmark-report-") && name.ends_with(".json")
            })
            .count();
        assert!(
            json_reports >= 1,
            "expected benchmark report artifacts despite gate failure"
        );
    }
}
