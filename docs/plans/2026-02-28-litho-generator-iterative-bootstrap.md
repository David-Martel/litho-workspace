# Litho Generator Iterative Bootstrap Runbook

Date: 2026-02-28

Purpose: bootstrap project documentation with `litho-generator.exe`, then iteratively improve quality with repeatable runs and captured validation artifacts.

## 1) Prerequisites

1. Rust toolchain available.
2. `target/release/litho-generator.exe` exists (or build it first).
3. One LLM provider is usable:
   - `ollama` with local models, or
   - `codexrs` provider/fallback where applicable.
4. A project config file (`litho.toml`) is available or CLI overrides are provided.

## 2) Binary Freshness Check

Run from workspace root:

```powershell
cargo run -p litho-generator -- --help
```

Expected flags include:
- `--incremental`
- `--format`

If the prebuilt binary differs from source help output, rebuild release binary before operational runs.

## 3) Full Bootstrap Run

Example with config file:

```powershell
target/release/litho-generator.exe `
  --project-path . `
  --output-path .\litho.docs `
  --config .\litho.toml `
  --format md
```

Expected outputs:
- section docs in output directory
- `__Litho_Summary_Detail__.md`
- `__Litho_Summary_Brief__.md`

## 4) Enable Iterative Improvement Signals

Set in `litho.toml`:

```toml
[quality]
min_score = 0.70
enforce_gate = false
regression_threshold = 0.05
# optional baseline_report_path = "path/to/validation-report.json"

[review]
enabled = true
min_review_score = 0.60
max_retries = 1
```

Then rerun generation and compare scores over time.

## 5) Incremental Loop

After code changes:

```powershell
target/release/litho-generator.exe `
  --incremental `
  --project-path . `
  --output-path .\litho.docs `
  --config .\litho.toml
```

If manifest metadata is incomplete or missing, generator may fall back to broad reruns. Treat this as a signal to improve manifest/change mapping quality.

## 6) Automated Loop Script

Use:

```powershell
pwsh -NoProfile -File .\scripts\litho-doc-bootstrap.ps1 `
  -ProjectPath . `
  -OutputPath .\litho.docs `
  -ConfigPath .\litho.toml `
  -Iterations 3 `
  -IncrementalAfterFirst
```

Artifacts written under:
- `.litho/runs/<timestamp>/run-summary.json`
- `.litho/runs/<timestamp>/run-summary.md`
- copied validation reports per run (if present)

## 7) Multi-Agent Coordination Pattern

For long-running doc quality work, mirror the context pattern used in external Claude setups:

1. Save per-session context summary.
2. Record agent registry (who changed what and why).
3. Keep an explicit latest pointer (`LATEST_CONTEXT.md` style).
4. Track unresolved blockers and quality deltas.

Recommended local location:
- `.claude/context/`

## 8) Operational KPIs

Track these for each run:

1. total duration
2. validation quality score
3. sections failing review threshold
4. incremental/full mode used
5. failure type (provider timeout, parser error, config error)

Minimum maturity target:
- 3 consecutive runs with stable or improving quality and no blocker failures.
