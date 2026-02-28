# Quality-First Documentation Framework Design

**Date:** 2026-02-28
**Status:** Approved
**Session:** 51+

## Problem Statement

litho-generator produces C4 architecture documentation via multi-agent LLM
orchestration, but has no systematic way to measure, benchmark, or enforce
documentation quality. Context window misconfiguration (Gemma3 defaults to 8K
despite supporting 128K) silently degrades output. Hardcoded parameters prevent
runtime tuning.

## Design Goals

1. **Externalize all parameters** to litho.toml / env vars (no hardcoded magic numbers)
2. **Fix context window** via native ollama-rs API with per-request `num_ctx`
3. **Multi-dimensional quality scoring** (deterministic + LLM-as-judge)
4. **Self-test capability** — generate docs for litho-workspace, score them
5. **Benchmark suite** — track quality and performance regressions over time
6. **Keep ollama-rs** as primary LLM framework

## Architecture

### WS1: Config Externalization & Context Window Fix

**Config hierarchy** (lowest wins):
```
Built-in defaults → litho.toml → env vars → CLI flags
```

**New configurable parameters** (currently hardcoded):

| Parameter | Current | Default | Config Key |
|-----------|---------|---------|------------|
| context_window | 32768 | model-dependent | `[llm] context_window` |
| max_tokens | 4096 | 4096 | `[llm] max_tokens` |
| temperature | 0.1 | 0.1 | `[llm] temperature` |
| max_parallels | 8 | 8 | `[llm] max_parallels` |
| quality_threshold | (none) | 0.0 | `[quality] min_score` |
| completeness_weight | 0.30 | 0.30 | `[quality] completeness_weight` |
| accuracy_weight | 0.30 | 0.30 | `[quality] accuracy_weight` |
| freshness_weight | 0.15 | 0.15 | `[quality] freshness_weight` |
| grounding_weight | 0.25 | 0.25 | `[quality] grounding_weight` |
| helpfulness_weight | (new) | 0.20 | `[quality] helpfulness_weight` |
| coherence_weight | (new) | 0.15 | `[quality] coherence_weight` |
| batch_source_budget | 50000 | 50000 | `[preprocessing] batch_source_budget_bytes` |
| small_file_threshold | 3072 | 3072 | `[preprocessing] small_file_threshold_bytes` |

**Context window model defaults** (auto-detected, overridable):

| Model Pattern | Default num_ctx |
|--------------|----------------|
| `gemma3:*` | 131072 |
| `qwen2.5-coder:*` | 131072 |
| `llama3*` | 131072 |
| `mistral*` | 32768 |
| `*` (fallback) | 32768 |

**Implementation**: In `ollama_native.rs`, the native ollama-rs API already
supports `num_ctx` via `ModelOptions`. Ensure ALL LLM calls (not just native
Ollama) respect the configured context window. For the OpenAI-compatible
provider path, inject `num_ctx` into the `options` field of the request body.

### WS2: Quality Scoring System (6 dimensions)

Extends existing `ContentValidator` in `generator/validator/mod.rs`.

#### Dimension 1: Completeness (deterministic, no LLM)

**Current**: Checks for 4-5 expected section headings.
**Enhanced**: AST-based symbol coverage.

```
Score = |Documented Symbols| / |Total Public Symbols|
```

Where "documented" means the symbol name appears in generated docs.
Uses litho-extract's Interface output as ground truth.

#### Dimension 2: Accuracy (deterministic, no LLM)

**Current**: Validates file references exist on disk.
**Enhanced**: Also validate dependency claims against AST import graph.

#### Dimension 3: Freshness (deterministic)

**Current**: Checks file paths still exist. Keep as-is.

#### Dimension 4: Grounding (deterministic + manifest)

**Current**: Cross-references tech claims against Cargo.toml/package.json.
Keep as-is.

#### Dimension 5: Helpfulness (LLM-as-judge, G-Eval style)

New. Uses the same local Ollama model as a judge with structured rubric:

```
Rate this documentation section on a 1-5 scale for each criterion:

SUMMARY QUALITY: Does the first sentence accurately capture the purpose?
DEPTH: Does it explain WHY, not just WHAT?
ACTIONABILITY: Could a new developer use this to start contributing?
EXAMPLES: Are usage examples or code references provided?

Evaluate step by step, then provide scores.
```

Score = average of 4 sub-scores, normalized to 0.0-1.0.

#### Dimension 6: Coherence (deterministic + LLM)

New. Two sub-checks:
1. **Terminology consistency** (deterministic): Extract entity names from all
   docs, flag inconsistencies (fuzzy match with Levenshtein distance < 3)
2. **Narrative coherence** (LLM judge): Rate structural flow 1-5

#### Quality Report Output

```
Quality Report for: litho-workspace
═══════════════════════════════════
Completeness:  0.87 (weight: 0.20) — 45/52 public symbols documented
Accuracy:      0.94 (weight: 0.20) — 32/34 file references valid
Freshness:     1.00 (weight: 0.10) — all paths exist
Grounding:     0.91 (weight: 0.15) — 21/23 tech claims verified
Helpfulness:   0.78 (weight: 0.20) — avg 3.9/5 across sections
Coherence:     0.85 (weight: 0.15) — 2 terminology inconsistencies
─────────────────────────────────
Overall Score: 0.88
Threshold:     0.70 ✓ PASS
```

### WS3: Test & Benchmark Framework

#### Unit Tests (target: 50+ new tests)

- Quality scorer: each dimension independently tested with synthetic docs
- Config parsing: all new config keys with defaults, overrides, validation
- Entity extraction: symbol name matching against doc content
- Context window: model-pattern detection, num_ctx propagation

#### Integration Test: Self-Documentation

Run litho-generator on the litho-workspace codebase itself:
1. Extract AST (litho-extract)
2. Generate docs (litho-generator with local Ollama)
3. Score docs (quality scorer)
4. Assert minimum quality threshold

This test requires Ollama running, so gate behind `#[cfg(feature = "integration")]`
or `#[ignore]` with env var opt-in.

#### Criterion Benchmarks

| Benchmark | Target | Metric |
|-----------|--------|--------|
| Token compression | 1000 source files | throughput (MB/s) |
| Quality scoring (deterministic) | 10 doc sets | latency (ms/doc) |
| AST completeness check | litho-workspace | latency (ms) |
| Entity extraction | 5 doc sections | latency (ms/section) |
| Markdown fixer | 100 documents | throughput (docs/s) |

### WS4: Prompt & Agent Improvements

Based on research findings:

1. **Chain-of-thought prompts**: Add "analyze step by step" instructions to
   research agents for more grounded reasoning
2. **Topological ordering**: Process modules in dependency order during
   composition (document dependencies first, use their docs as context)
3. **Anti-hallucination strengthening**: Add explicit "cite file:line for every
   claim" instructions to compose agents
4. **Few-shot examples**: Add 2-3 exemplar C4 doc sections to compose agent
   prompts for consistent formatting

## Implementation Order

1. Config externalization (all hardcoded params → litho.toml)
2. Context window fix (ollama-rs num_ctx propagation)
3. Deterministic quality dimensions (completeness, coherence)
4. LLM-as-judge dimensions (helpfulness, entity tracing)
5. Benchmark suite (criterion + integration)
6. Prompt improvements (CoT, topological ordering)
7. Self-documentation test (litho-workspace as test corpus)

## Success Criteria

- All parameters configurable via litho.toml with validation
- Context window correctly set to 128K+ for Gemma3/Qwen models
- Quality score reproducible and deterministic for non-LLM dimensions
- Self-documentation of litho-workspace scores >= 0.70 overall
- Benchmark suite runs in < 5 minutes (excluding LLM calls)
- 50+ new tests, all passing
