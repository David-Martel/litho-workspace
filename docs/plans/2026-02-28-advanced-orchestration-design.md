# Advanced Orchestration & Optimization Design

**Date:** 2026-02-28
**Status:** Implementation Phase
**Session:** 52
**Prerequisite:** Quality-First Framework (completed same session)

## Problem Statement

litho-generator has solid fundamentals (4-stage pipeline, async-first, dual-key
caching, incremental mode) but lacks advanced ML patterns: MoE-style model
routing, secondary agent review, memory versioning, and adaptive optimization.
The user requests these capabilities along with a critical review of the
Session 52 quality framework implementation.

## WS1: MoE-Style Model Routing

**Current:** All agents use the same `model_efficient` / `model_powerful` pair.
Model selection is purely size-based (>32KB → powerful model).

**Enhancement:** Route agents to optimal models based on task complexity:

```
[llm.routing]
# Model preferences by agent role
preprocessing_model = "model_efficient"   # Fast, high-volume
research_model = "model_efficient"        # Analysis tasks
compose_model = "model_powerful"          # Creative writing
review_model = "model_efficient"          # Validation tasks
```

Implementation: Add `model_preference` field to `StepForwardAgent` trait
that returns one of `{Efficient, Powerful, Auto}`. The `agent_executor`
checks this preference before calling `evaluate_befitting_model`.

## WS2: Secondary Review Agent

**Current:** Generated docs go directly to outlet with no post-generation
quality check (except the deterministic validator).

**Enhancement:** After each compose agent produces a section, a lightweight
review agent checks it for:
1. Factual grounding (are claims backed by research data?)
2. Structural consistency (does it follow the C4 format?)
3. Completeness (does it reference all relevant modules?)

The review agent uses the same LLM (efficient model) with a targeted prompt.
If the review score is below threshold, the section is re-generated with
the review feedback injected into the prompt.

```rust
pub struct ReviewConfig {
    /// Enable secondary review pass
    pub enabled: bool,
    /// Minimum review score (0.0-1.0) to accept without re-generation
    pub min_review_score: f64,
    /// Maximum re-generation attempts
    pub max_retries: u32,
}
```

## WS3: Memory Enhancement (Local + Global)

**Current:** Flat HashMap with `scope:key` namespacing. Write-once, no
versioning, no semantic queries.

**Enhancement:**
1. **Scoped memory tiers**: Agent-local (per-agent scratch), pipeline-global
   (shared across all agents), persistent (survives across runs)
2. **Inference accumulation**: Each agent can append "learned facts" that
   downstream agents can query
3. **Memory digest**: At each stage boundary, produce a compressed summary
   of accumulated knowledge for context injection

## WS4: Compression & Semantic Overlapping

**Current:** Two-stage compression (lossless strip + LLM-based), 50KB batch
budget, no deduplication.

**Enhancement:**
1. **Adaptive compression**: Vary threshold based on model context window
2. **Semantic deduplication**: When composing docs, detect and merge
   overlapping content across sections
3. **Hierarchical summarization**: For very large codebases, summarize
   at module level first, then compose from summaries

## Implementation Priority

| Feature | Impact | Effort | Priority |
|---------|--------|--------|----------|
| Model routing preferences | High | Low | P0 |
| Secondary review agent | High | Medium | P0 |
| Memory inference accumulation | Medium | Medium | P1 |
| Adaptive compression thresholds | Medium | Low | P1 |
| Semantic dedup in compose | Medium | High | P2 |

## Implementation Order

1. Model routing preferences (config + trait extension)
2. Review agent framework (new agent + compose integration)
3. Memory digest at stage boundaries
4. Adaptive compression thresholds
5. Critical review of Session 52 implementation
