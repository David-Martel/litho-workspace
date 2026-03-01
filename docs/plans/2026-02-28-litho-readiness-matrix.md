# Litho Workspace Readiness Matrix (A: Works / B: Works Well)

Date: 2026-02-28
Scope: `C:\codedev\litho-workspace` with context from:
- `.\.claude\context\*`
- `C:\codedev\david-t-martel\.claude\*`
- `C:\Users\david\.claude\*`

## Executive Summary

Current status:
- A) **Can work end-to-end**: **Mostly yes**, with environmental caveats.
- B) **Works well and repeatably**: **Not yet**. Key hardening and integration gaps remain.

Most important near-term blockers:
1. Local build/check reliability is unstable due `sccache` startup failure.
2. Built release binary is stale relative to source behavior.
3. Incremental mode has partial plumbing but weak manifest intelligence.
4. Validation/quality gate code exists but is not fully integrated as an enforced release gate.

## A) What Is Functionally Working

### Workspace and binaries
- Workspace structure is coherent and multi-crate integration is in place (`Cargo.toml` workspace includes `crates/*` plus external codex-rs crates).
- Built binaries exist in `target/release/`, including:
  - `litho-generator.exe`
  - `litho.exe`
  - `litho-book.exe`
  - `litho-qmd-cli.exe`
  - `litho-qmd-mcp.exe`

### Generator pipeline architecture
- `litho-generator` source implements a 4-stage flow in [`workflow.rs`](/C:/codedev/litho-workspace/crates/litho-generator/src/generator/workflow.rs):
  - preprocess
  - research
  - compose
  - output
- Research and compose support selective execution paths (`execute_research_pipeline_selective`, `execute_selective`).
- Secondary review loop exists and is configurable in [`compose/mod.rs`](/C:/codedev/litho-workspace/crates/litho-generator/src/generator/compose/mod.rs).

### LLM/provider flexibility
- Provider abstraction supports OpenAI-compatible, Anthropic, Gemini, Ollama, and CodexRs fallback path (`providers.rs`, `codex_provider.rs`).
- `LLMProvider` already accepts `codexrs`, `codex-rs`, and `codex` labels in config parsing.

### CI skeleton
- CI workflow (`.github/workflows/ci.yml`) builds/tests/lints.
- Docs workflow (`.github/workflows/docs.yml`) runs full generation on push and incremental generation on PR.

## A) Gaps That Still Prevent Reliable “It Works” Behavior

### 1. Build reliability regression (environment)
- `cargo check -p litho-generator` failed due `sccache` server startup port exclusion.
- This blocks a dependable local verification loop unless wrapper/port is adjusted.

### 2. Runtime binary freshness mismatch
- Existing `target/release/litho-generator.exe` help output was stale (missing newer flags).
- Source path via `cargo run -p litho-generator -- --help` shows expected flags (`--incremental`, `--format`).
- Impact: users can run old binaries and assume new source behavior exists when it does not.

### 3. CLI flags parsed but unused
- `--skip-preprocessing`, `--skip-research`, `--skip-documentation` are defined in [`cli.rs`](/C:/codedev/litho-workspace/crates/litho-generator/src/cli.rs) but are not consumed in workflow execution paths.
- Impact: operator intent is silently ignored.

### 4. Validation gate integration incomplete
- Quality gate helper exists in [`quality_gate.rs`](/C:/codedev/litho-workspace/crates/litho-generator/src/generator/quality_gate.rs) but is not called from workflow.
- Impact:
  - `baseline_report_path` regression logic is effectively inactive.
  - `enforce_gate`/`min_score` cannot fail pipeline in real runs.

### 5. Incremental intelligence incomplete
- Manifest model exists (`manifest.rs`) and change detector exists (`change_detector.rs`), but runtime manifest population is shallow in workflow.
- `file_hashes` and rich per-agent module inputs are not fully materialized during normal runs.
- Impact: incremental runs can degrade to over-broad reruns and weak affected-agent targeting.

### 6. Config validation not enforced at startup
- `QualityConfig::validate()` exists but no centralized runtime call enforces it after load/merge.
- Impact: invalid weight sums or thresholds can slip through and surprise operators later.

## B) What Must Exist for “Works Well”

## P0 (Required)

1. **Deterministic execution environment**
- Make `cargo check/test` resilient when `sccache` is unavailable (document bypass, optional wrapper, or stable port assignment).

2. **Single-source runtime semantics**
- Ensure operator-facing docs/scripts always run either:
  - fresh `cargo run -p litho-generator -- ...`, or
  - verified up-to-date `target/release/litho-generator.exe`.

3. **Wire quality gate into both workflow modes**
- Invoke `process_validation_report()` in:
  - full launch
  - incremental launch
- Preserve `validation-report.json` in final output directory.

4. **Honor CLI skip flags or remove them**
- Implement skip behavior in workflow, or remove flags to avoid false affordances.

## P1 (High-value quality)

1. **Manifest enrichment**
- Record file hashes and meaningful per-agent input file sets for better change mapping.

2. **Incremental re-run precision**
- Refine `map_files_to_agents()` policy to avoid all-or-nothing compose reruns when only one domain changed.

3. **Configuration hardening**
- Validate quality weights/thresholds and context settings once at startup with explicit errors.

4. **Docs workflow trust model**
- Keep `continue-on-error: true` only if paired with explicit quality/status artifact and clear failure signaling.

## P2 (Scale and operator productivity)

1. Improve preprocessing throughput (still dominant runtime cost in prior sessions).
2. Add run-history artifacts for quality trend tracking across iterations.
3. Standardize context snapshots (`LATEST_CONTEXT.md`, index, per-run summaries) for multi-agent continuity.

## Adoptable Patterns from External Claude Contexts

Useful patterns already proven in:
- `C:\Users\david\.claude\agents\context-manager.md`
- `C:\Users\david\.claude\agents\codex-orchestrator.md`
- `C:\codedev\david-t-martel\.claude\commands\tools\multi-agent-review.md`

Recommended adaptations for litho-workspace:
1. Run-level context snapshots in `.claude/context/` with:
   - latest pointer
   - agent/work registry
   - explicit blocker list
2. Iterative “generate -> validate -> review -> regenerate” loop as a standard operating procedure.
3. Explicit reviewer criteria (grounding, structure, completeness) and regression thresholds tracked per run.

## Immediate Next Actions

1. Use `scripts/litho-doc-bootstrap.ps1` (added alongside this report) for repeatable generation loops.
2. Make quality gate execution path mandatory for both full and incremental workflow.
3. Tighten manifest/change detection integration before claiming incremental performance targets.
