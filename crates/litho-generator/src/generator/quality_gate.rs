//! Quality gate — validation persistence, regression detection, and enforcement.
//!
//! Extracted from `workflow.rs` so that both `launch()` and `launch_incremental()`
//! share a single code path for post-validation processing.

use anyhow::Result;

use crate::generator::context::GeneratorContext;
use crate::generator::validator::ValidationReport;

/// Result of comparing the current score against a baseline.
#[derive(Debug, Clone, PartialEq)]
pub enum RegressionResult {
    /// No baseline report was configured or found.
    NoBaseline,
    /// Current score meets or exceeds the baseline within the threshold.
    Passed {
        baseline_score: f64,
        current_score: f64,
    },
    /// Current score dropped below the baseline beyond the allowed threshold.
    Regressed {
        baseline_score: f64,
        current_score: f64,
        delta: f64,
    },
}

/// Persist the validation report, run regression checks, and enforce the quality gate.
///
/// 1. Stores the report to memory at `"validation"/"report"`.
/// 2. Writes JSON to `{output_path}/validation-report.json` (warns on I/O error).
/// 3. If `baseline_report_path` is set, loads it and compares scores.
/// 4. If `enforce_gate && min_score > 0.0 && quality_score < min_score`, returns `Err`.
/// 5. Regression + gate failures only error when `enforce_gate = true`; otherwise warn.
pub async fn process_validation_report(
    context: &GeneratorContext,
    report: &ValidationReport,
) -> Result<()> {
    // 1. Store report in memory
    context
        .store_to_memory("validation", "report", report.clone())
        .await?;

    // 2. Write JSON to output_path
    let output_file = context.config.output_path.join("validation-report.json");
    match serde_json::to_string_pretty(report) {
        Ok(json) => {
            if let Err(e) = tokio::fs::write(&output_file, &json).await {
                eprintln!(
                    "  Warning: Failed to write validation report to {}: {}",
                    output_file.display(),
                    e
                );
            }
        }
        Err(e) => {
            eprintln!("  Warning: Failed to serialize validation report: {}", e);
        }
    }

    let qc = &context.config.quality;

    // 3. Regression check
    let regression = check_regression(report, qc).await;
    match &regression {
        RegressionResult::Regressed {
            baseline_score,
            current_score,
            delta,
        } => {
            let msg = format!(
                "Quality regression detected: baseline={:.1}%, current={:.1}%, delta={:.1}% (threshold={:.1}%)",
                baseline_score * 100.0,
                current_score * 100.0,
                delta * 100.0,
                qc.regression_threshold * 100.0,
            );
            if qc.enforce_gate {
                return Err(anyhow::anyhow!("{}", msg));
            }
            eprintln!("  Warning: {}", msg);
        }
        RegressionResult::Passed {
            baseline_score,
            current_score,
        } => {
            println!(
                "  Regression check passed: baseline={:.1}%, current={:.1}%",
                baseline_score * 100.0,
                current_score * 100.0,
            );
        }
        RegressionResult::NoBaseline => {}
    }

    // 4. Quality gate enforcement
    if qc.enforce_gate && qc.min_score > 0.0 && report.quality_score < qc.min_score {
        return Err(anyhow::anyhow!(
            "Quality gate failed: score={:.1}% < min_score={:.1}%",
            report.quality_score * 100.0,
            qc.min_score * 100.0,
        ));
    }

    Ok(())
}

/// Load a baseline report (if configured) and compare scores.
async fn check_regression(
    current: &ValidationReport,
    qc: &crate::config::QualityConfig,
) -> RegressionResult {
    let baseline_path = match &qc.baseline_report_path {
        Some(p) => p,
        None => return RegressionResult::NoBaseline,
    };

    let json = match tokio::fs::read_to_string(baseline_path).await {
        Ok(j) => j,
        Err(e) => {
            eprintln!(
                "  Warning: Could not read baseline report at {}: {}",
                baseline_path.display(),
                e
            );
            return RegressionResult::NoBaseline;
        }
    };

    let baseline: ValidationReport = match serde_json::from_str(&json) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  Warning: Could not parse baseline report: {}", e);
            return RegressionResult::NoBaseline;
        }
    };

    let delta = baseline.quality_score - current.quality_score;
    if delta > qc.regression_threshold {
        RegressionResult::Regressed {
            baseline_score: baseline.quality_score,
            current_score: current.quality_score,
            delta,
        }
    } else {
        RegressionResult::Passed {
            baseline_score: baseline.quality_score,
            current_score: current.quality_score,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::validator::ValidationReport;

    fn make_report(quality_score: f64) -> ValidationReport {
        ValidationReport {
            findings: Vec::new(),
            quality_score,
            completeness_score: quality_score,
            accuracy_score: quality_score,
            freshness_score: quality_score,
            grounding_score: quality_score,
            coherence_score: quality_score,
            helpfulness_score: quality_score,
            section_scores: Vec::new(),
            generated_at: String::new(),
        }
    }

    #[test]
    fn test_regression_no_baseline() {
        let qc = crate::config::QualityConfig::default();
        let current = make_report(0.8);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(check_regression(&current, &qc));
        assert_eq!(result, RegressionResult::NoBaseline);
    }

    #[test]
    fn test_regression_bad_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("baseline.json");
        std::fs::write(&path, "not json").unwrap();

        let qc = crate::config::QualityConfig {
            baseline_report_path: Some(path),
            ..Default::default()
        };
        let current = make_report(0.8);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(check_regression(&current, &qc));
        assert_eq!(result, RegressionResult::NoBaseline);
    }

    #[test]
    fn test_regression_passed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("baseline.json");
        let baseline = make_report(0.80);
        std::fs::write(&path, serde_json::to_string(&baseline).unwrap()).unwrap();

        let qc = crate::config::QualityConfig {
            baseline_report_path: Some(path),
            regression_threshold: 0.05,
            ..Default::default()
        };
        let current = make_report(0.78); // delta = 0.02, within threshold
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(check_regression(&current, &qc));
        assert!(matches!(result, RegressionResult::Passed { .. }));
    }

    #[test]
    fn test_regression_detected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("baseline.json");
        let baseline = make_report(0.90);
        std::fs::write(&path, serde_json::to_string(&baseline).unwrap()).unwrap();

        let qc = crate::config::QualityConfig {
            baseline_report_path: Some(path),
            regression_threshold: 0.05,
            ..Default::default()
        };
        let current = make_report(0.70); // delta = 0.20, exceeds threshold
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(check_regression(&current, &qc));
        match result {
            RegressionResult::Regressed { delta, .. } => {
                assert!((delta - 0.20).abs() < 0.001);
            }
            _ => panic!("Expected Regressed, got {:?}", result),
        }
    }

    #[test]
    fn test_regression_boundary_exact_threshold() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("baseline.json");
        let baseline = make_report(0.85);
        std::fs::write(&path, serde_json::to_string(&baseline).unwrap()).unwrap();

        let qc = crate::config::QualityConfig {
            baseline_report_path: Some(path),
            regression_threshold: 0.05,
            ..Default::default()
        };
        // delta = 0.05, exactly at threshold -> should PASS (> not >=)
        let current = make_report(0.80);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(check_regression(&current, &qc));
        assert!(matches!(result, RegressionResult::Passed { .. }));
    }

    #[test]
    fn test_regression_file_not_found() {
        let qc = crate::config::QualityConfig {
            baseline_report_path: Some(std::path::PathBuf::from("/nonexistent/baseline.json")),
            ..Default::default()
        };
        let current = make_report(0.8);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(check_regression(&current, &qc));
        assert_eq!(result, RegressionResult::NoBaseline);
    }

    #[tokio::test]
    async fn test_gate_pass_when_not_enforced() {
        // With enforce_gate=false (default), quality_score < min_score should NOT error
        let config = crate::config::Config {
            quality: crate::config::QualityConfig {
                min_score: 0.99,
                enforce_gate: false,
                ..Default::default()
            },
            ..crate::config::Config::default()
        };
        let llm_client = crate::llm::client::LLMClient::new(config.clone()).unwrap();
        let context = GeneratorContext {
            llm_client,
            config,
            cache_manager: std::sync::Arc::new(tokio::sync::RwLock::new(
                crate::cache::CacheManager::new(Default::default(), Default::default()),
            )),
            memory: std::sync::Arc::new(tokio::sync::RwLock::new(crate::memory::Memory::new())),
        };

        let report = make_report(0.50);
        // Should succeed even though 0.50 < 0.99 — gate not enforced
        let result = process_validation_report(&context, &report).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_gate_fails_when_enforced() {
        let dir = tempfile::tempdir().unwrap();
        let config = crate::config::Config {
            output_path: dir.path().to_path_buf(),
            quality: crate::config::QualityConfig {
                min_score: 0.90,
                enforce_gate: true,
                ..Default::default()
            },
            ..crate::config::Config::default()
        };
        let llm_client = crate::llm::client::LLMClient::new(config.clone()).unwrap();
        let context = GeneratorContext {
            llm_client,
            config,
            cache_manager: std::sync::Arc::new(tokio::sync::RwLock::new(
                crate::cache::CacheManager::new(Default::default(), Default::default()),
            )),
            memory: std::sync::Arc::new(tokio::sync::RwLock::new(crate::memory::Memory::new())),
        };

        let report = make_report(0.50);
        let result = process_validation_report(&context, &report).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Quality gate failed"));
    }
}
