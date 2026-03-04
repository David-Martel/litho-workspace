# Current Plan Snapshot (2026-03-03)

## What Works

- Core pipeline (`litho-generator`) is implemented and integrated: preprocess → research → compose → output, with quality validation code present and incremental mode scaffolded.
- Quality framework and content guardrails have been added (content validator, markdown fixer, regression comparison, quality score scaffolding).
- Provider abstraction is stable (OpenAI-compatible, Anthropic, Gemini, Ollama, CodexRs fallback/selection path).
- CI and generation docs are present; bootstrap scripts exist for iterative reruns.
- Test base is substantial and recent commits show active work on build reliability and quality hardening.

## What Remains

- Close CLI skip-flag gap (`--skip-preprocessing`, `--skip-research`, `--skip-documentation`) so CLI behavior matches documented intent.
- Finalize incremental intelligence: stronger manifest integrity checks, richer file→agent mapping, and safer partial rerun behavior.
- Make quality gates and regression baselines enforceable as release gates with strict fail behavior.
- Improve preprocess throughput; precomputation cache warming still only partially addresses major bottleneck.
- Harden config validation at startup and keep binaries/build artifacts version-coupled.
- Expand unit/integration coverage in TODO backlog (`structure_extractor`, `manifest`, `change_detector`, `litho-cli`, provider parsing paths).

## Tracking References

- `TODO.md` — authoritative work backlog with P0/P1/P2 items.
- `docs/plans/2026-02-28-litho-readiness-matrix.md` — reliability and quality blockers.
- `CLAUDE.md` — architecture, command recipes, and operational gotchas.
- `.github/workflows` and `scripts/*` — hardening points for reproducibility.
