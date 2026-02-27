# david-t-martel Context — 2026-02-27

## Session 46: ollama-rs Native Provider + Gemma3 Pipeline

### State Summary

Successfully integrated ollama-rs native provider into litho-workspace, replacing rig-core's
OpenAI-compat layer for Ollama. Ran full pipeline with Gemma3 12B-IT-QAT on david-t-martel repo — 9
docs generated in ~98 minutes with dramatically improved quality (correct project identification,
tech stack, target user). Added configurable `context_window` for 128K Gemma3 context. Two repos
have uncommitted changes: david-t-martel (89 files) and litho-workspace (33 files).

### Recent Changes

**litho-workspace (33 files, +966/-46):**

- `ollama_native.rs` (NEW): Native Ollama provider using ollama-rs 0.3 — chat(), extract(),
  5-strategy JSON parse cascade
- `mod.rs`: Wire native client into LLMClient — intercept extract/prompt/prompt_without_react
- `config.rs`: Add `context_window: u32` field (default 32768, configurable in litho.toml)
- `Cargo.toml`: Add ollama-rs 0.3 and url 2 dependencies
- `.cargo/config.toml`: Disable LTO (rustc stack overflow on rig-core 0.23)
- Sprint 1-4 changes from session 45 (prompt quality, incremental mode, HTML outlet, CI)

**david-t-martel (89 files, +15911/-13991):**

- `litho.toml`: Updated to use litho-gemma3:12b, context_window=131072
- Generated docs in `docs/auto/litho_docs/` (9 files)
- Various fact/template/tool updates from previous sessions
- MCP server updates (google-calendar, google-drive, google-photos, shared auth)

### Work in Progress

- Token-aware preprocessing (task #7) — strip comments, compress whitespace in sources.rs
- Codex-rs as primary provider for faster generation with frontier models
- ollama-rs Coordinator for Gemma3 tool calling

### Decisions

| ID | Topic | Decision | Rationale | |----|-------|----------|-----------| | dec-046-1 | Native
Ollama provider | ollama-rs 0.3 bypassing rig-core | num_ctx control, Gemma3 support, native API
access | | dec-046-2 | context_window config | Configurable in litho.toml | Gemma3 supports 128K,
qwen only 32K | | dec-046-3 | LTO disabled | Commented out in .cargo/config.toml | rustc stack
overflow on rig-core 0.23 | | dec-046-4 | prompt() interception | Route to prompt_without_react for
Ollama | ReAct tools not essential for doc generation |

### Agent Registry

| Agent | Task | Status | Notes | |-------|------|--------|-------| | rust-pro (session 44) |
CodexRs fallback + serde hardening | Complete | 368 tests passing | | rust-pro (session 45) |
Sprints 1-4 implementation | Complete | 396 tests passing | | search-specialist (session 46) |
Gemma3 model research | Complete | Found QAT variants, 128K context | | Explore (session 46) |
Ollama API construction analysis | Complete | Found model_powerful dead code |

### Performance Baseline

| Metric | Value | |--------|-------| | Pipeline total time | 5892s (~98 min) | | Preprocessing (127
files) | 5181s (87.9%) | | Research | 398s (6.8%) | | Documentation | 312s (5.3%) | | Cache hit rate
(fresh) | 8.9% | | Gemma3 VRAM | 16.5 GB (100% GPU) |

### Next Steps

1. Commit-cluster both repos
1. Wire codex-rs as primary provider option for faster generation
1. Add ollama-rs Coordinator tool calling for Gemma3
1. Token-aware preprocessing to reduce per-file prompt size
1. Parallel preprocessing calls (currently sequential despite max_parallels config)
