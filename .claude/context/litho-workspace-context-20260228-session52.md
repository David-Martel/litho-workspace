# Litho Workspace Context — Session 52

**Context ID:** ctx-litho-workspace-20260228-session52
**Created:** 2026-02-28
**Branch:** main @ 74f0f3d
**Session:** 52
**Tests:** 768 passing (litho-core + litho-extract + litho-generator)

## Summary

Session 52 implemented the quality-first documentation framework with advanced
orchestration features: 6-dimension quality scoring, config externalization,
context window auto-detection, MoE-style model routing, secondary review agent,
memory digest system, chain-of-thought compose prompts, and criterion benchmarks.
123 new tests added (645→768). 3 commits pushed to main.

## Recent Changes

| File | Change |
|------|--------|
| config.rs | +QualityConfig, PreprocessingConfig, ReviewConfig, model_default_context_window(), resolve_context_window() |
| validator/mod.rs | 6-dimension scoring, symbol coverage, coherence, helpfulness LLM-judge |
| step_forward_agent.rs | ModelPreference enum (Efficient/Powerful/Auto) |
| agent_executor.rs | Model preference routing in extract/prompt/prompt_with_tools |
| llm/client/mod.rs | extract_with_models(), prompt_without_react_with_model(), prompt_with_model() |
| llm/client/utils.rs | resolve_model_for_agent() |
| ollama_native.rs | resolve_context_window() in build_options() |
| code_analyze.rs | Reads from config.preprocessing instead of hardcoded constants |
| 7 research agents | ModelPreference::Efficient override |
| 6 compose agents | ModelPreference::Powerful + CoT reasoning + grounding rules + formatting examples |
| review_agent.rs | NEW — secondary review agent with 3-strategy JSON extraction |
| compose/mod.rs | Review pass integration in execute() and execute_selective() |
| memory/mod.rs | append_inference(), get_inferences(), digest() |
| context.rs | Async convenience methods for inference/digest |
| workflow.rs | Stage-boundary digests (preprocess→research transitions) |
| benches/quality_benchmarks.rs | NEW — 6 groups, 20 benchmark cases |
| tests/model_preference_tests.rs | NEW — 9 tests for model routing |

## Architecture Decisions

### Quality Scoring (6 Dimensions)
- Completeness = 60% section structure + 40% AST symbol coverage
- Helpfulness = LLM-as-judge G-Eval with 4 sub-criteria (1-5 scale)
- Coherence = backtick term extraction + normalized grouping, Rust casing excluded
- Weights: completeness=0.20, accuracy=0.20, freshness=0.10, grounding=0.20, coherence=0.15, helpfulness=0.15

### MoE Model Routing
- Research agents → model_efficient (analytical, high-volume)
- Compose agents → model_powerful (creative synthesis)
- Default → Auto (size-based routing via evaluate_befitting_model)

### Secondary Review Agent
- Disabled by default (review.enabled = false)
- Requires Ollama provider
- 3 criteria: grounding (1-5), structure (1-5), completeness (1-5)
- min_review_score = 0.6, max_retries = 1

### Memory Digest
- Inferences stored as Vec<String> at {scope}:_inferences
- Digests produced at preprocess→research and research→compose boundaries
- 200-char value previews, capped at 20 inferences per digest

## Agent Work Registry

| Agent | Task | Files | Status |
|-------|------|-------|--------|
| rust-pro #1 | Config externalization + context window | config.rs, ollama_native.rs, code_analyze.rs, tests | Complete |
| rust-pro #2 | Enhanced quality dimensions | validator/mod.rs (coherence, symbol coverage) | Complete |
| rust-pro #3 | Prompt improvements | 5 compose agent files | Complete |
| rust-pro #4 | LLM-as-judge helpfulness | validator/mod.rs (helpfulness, parse_scores) | Complete |
| rust-pro #5 | Criterion benchmarks | benches/quality_benchmarks.rs, Cargo.toml | Complete |
| rust-pro #6 | Model routing | step_forward_agent.rs, agent_executor.rs, utils.rs, 11 agents | Complete |
| rust-pro #7 | Critical review analysis | validator/mod.rs (7 issues identified) | Complete |
| rust-pro #8 | Critical review fixes | validator/mod.rs (word boundary, casing, helpfulness) | Complete |
| rust-pro #9 | Secondary review agent | review_agent.rs, compose/mod.rs, config.rs | Complete |
| rust-pro #10 | Memory digest | memory/mod.rs, context.rs, workflow.rs | Complete |

## Roadmap

### Immediate (next session)
- Push 4 commits to origin/main
- Run full integration test against a real repository
- Wire review agent feedback into compose re-generation loop

### This Week
- Adaptive compression thresholds (vary by model context window)
- Semantic deduplication in compose output
- Hierarchical summarization for very large codebases

### Tech Debt
- OllamaNativeClient::chat() model parameter could be empty string (validated now but could be typed)
- check_symbol_coverage O(n*m) — could use Aho-Corasick for large symbol sets
- imports_granularity = Item requires nightly — consider removing from rustfmt.toml
