# Codex-RS + Ollama + AST Optimization Plan (2026-03-05)

## Scope
This plan targets remaining high-impact blockers in `litho-generator` and `litho-extract`:
- codex-rs primary/hybrid provider readiness
- Ollama runtime robustness and latency stability
- AST/walker acceleration for large repositories

## Current State
- CodexRs is wired as a provider enum + extraction fallback path.
- Native Ollama path now supports `/api/tags` discovery, optional pull, and startup warmup.
- Incremental manifest now records `file_hashes` + module metadata.
- `litho-extract` file analysis now runs in parallel with deterministic ordering.

## Proposal

### 1. Codex-rs primary/hybrid rollout
1. Fix model routing correctness in prompt paths (`prompt_with_model` + AgentBuilder model wiring).
2. Extend codex fallback from extract-only to prompt/review paths.
3. Add explicit codex invocation controls:
   - model
   - cwd/project path
   - optional JSON schema output for structured extraction
4. Normalize codex binary env variables across crates (`CODEX_BINARY_PATH` and `CODEX_BIN`).

Benefits:
- Better resilience when local Ollama fails.
- Higher quality for compose/review stages when frontier models are configured.

Trade-offs:
- More provider-path branching and test surface.
- Requires clear operator docs for local vs remote codex model selection.

### 2. Ollama robustness hardening
1. Add explicit timeout wrappers around all native ollama-rs calls.
2. Add bounded parse/quality retry rounds (best-effort final acceptance).
3. Add adaptive `num_ctx` sizing based on prompt size and workload.
4. Keep model warmup/pull optional and disabled by default in CI contexts.

Benefits:
- Fewer long hangs and malformed response failures.
- Lower p95 latency variance on mixed repo sizes.

Trade-offs:
- Slightly higher complexity in request option selection.
- More tunables to document.

### 3. AST/walker acceleration phase
1. Batch ast-grep extraction by language/pattern (single command over many files).
2. Add graceful degradation when `sg` is missing/malformed output.
3. Add walker chunking/merge strategy for very large repos.

Benefits:
- Lower process spawn overhead.
- Better throughput for incremental detection and semantic enrichment.

Trade-offs:
- Additional mapping logic from batched AST results back to source files.

## Phased Execution
- Phase A (low risk): provider model routing fix + codex fallback extension.
- Phase B: Ollama timeout/retry/adaptive context controls.
- Phase C: AST batch mode + walker chunking.

## Validation Gates
- `cargo test -p litho-generator --lib`
- `cargo test -p litho-extract --lib`
- Targeted integration: incremental run with partial file changes + provider failover scenario.
