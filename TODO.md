# Litho Workspace TODOs

Last audited against the working tree: 2026-04-08

This backlog reflects the code currently in the repository, not the older plan
state. `GOALS.md` remains the target-state contract; this file tracks the
remaining implementation work needed to get there.

## Observed State

- `litho` now exposes `extract`, `generate`, `init`, `index`, `status`,
  `serve`, `validate`, `search`, `query`, and `vsearch`.
- The single-entrypoint goal is still incomplete: `litho generate` uses the
  `litho-codex` path, while the full preprocess -> research -> compose ->
  validate pipeline still lives in `litho-generator`.
- QMD simplification has started but is not finished:
  - `litho-qmd-core` is now mostly a compatibility re-export over
    `litho-qmd-storage`.
  - `litho-qmd-storage` has started to split into modules (`auto_store.rs`,
    `config.rs`, `model.rs`, `service.rs`, `traits.rs`, `sqlite_impl.rs`), but
    `lib.rs` is still large and still carries backend logic.
  - `litho-qmd-llm` remains a separate crate, but it now imports
    `litho-qmd-storage` directly.
- QMD runtime config is still loaded from `qmd.config.json`,
  `config/qmd.config.json`, and `.env` candidates. It has not been folded into
  `litho.toml`.
- Incremental generation is implemented, but it is still manifest + git diff +
  input-file mapping driven. It does not yet operate on AST- or symbol-level
  deltas, and it does not preserve section-level merge provenance.
- Extraction coverage is uneven:
  - `litho-extract` has tree-sitter-backed interface/dependency extraction for
    Rust, TypeScript, Python, and C#.
  - Discovery and generator preprocessing already recognize additional
    languages, but that support is not yet matched by the AST extraction path.
- The workspace already has meaningful regression infrastructure:
  - snapshot coverage for `litho-extract`,
  - provider fallback tests in `litho-generator`,
  - deprecation coverage for direct `litho-qmd-cli` use,
  - and quality-gate wiring in `litho-generator`.

## Priority Order

1. Collapse documentation generation onto one user-facing workflow.
2. Finish the QMD crate/config consolidation that has already started.
3. Upgrade generator orchestration and QMD I/O so async boundaries are explicit.
4. Harden incremental regeneration beyond file-level git diff.
5. Align language support claims with the actual extraction stack.
6. Add new surface area only after the above architecture is stable.

## Completed Since The Earlier Backlog

- [x] `litho init` scaffolds `litho.toml` and `.litho/`.
- [x] `litho index` wraps QMD add + ingest + embed from the primary CLI.
- [x] `litho-generator` exposes `--min-quality` and `--enforce-gate`.
- [x] Direct `litho-qmd-cli` usage now emits a migration warning.
- [x] PostgreSQL is feature-gated in `litho-qmd-storage`.
- [x] `pdf-extract` is feature-gated in `litho-generator`.
- [x] `litho-qmd-core` has already been reduced to a compatibility layer over
  `litho-qmd-storage`.
- [x] The public QMD entrypoints (`litho`, `litho-qmd-cli`, `litho-qmd-mcp`)
  now depend on `litho-qmd-storage` directly instead of routing through
  `litho-qmd-core`.
- [x] `litho-qmd-llm` now depends on `litho-qmd-storage` directly instead of
  routing through `litho-qmd-core`.
- [x] QMD can start from `[qmd]` in `litho.toml`, while legacy
  `qmd.config.json` values still backfill missing settings.
- [x] The shared bounded-parallel helper in `litho-generator` now uses
  `JoinSet`, and `knowledge_sync` no longer carries its own `join_all` +
  semaphore fan-out.
- [x] Snapshot/provider/cache regression coverage is materially better than it
  was when the original TODO list was written.

## P0: Unify The User-Facing Workflow

Maps to GOALS: 1, 3, 8, 10

### One generation path

- [ ] Decide whether `litho-generator` becomes an internal engine behind
  `litho generate`, or whether the generator binary remains but `litho` becomes
  a strict front door over the same pipeline.
- [ ] Add a single integration seam so `litho generate` can invoke the full
  preprocess -> research -> compose -> validate engine without leaving users on
  a second primary workflow.
- [ ] Keep the Codex-only generation path only as an explicit advanced mode, or
  remove it if it no longer represents the supported architecture.
- [ ] Add CLI coverage proving that the primary `litho generate` path exposes
  incremental mode, quality-gate behavior, and output-format selection through
  one surface.
- [ ] Make `litho status` and related guidance point to the primary workflow
  instead of telling users to run `litho-generator` directly.
- [ ] Keep benchmark and low-level operational commands available, but make them
  advanced surfaces instead of parallel user journeys.

### Config convergence

- [ ] Decide whether `litho-core::LithoConfig` should become the single shared
  config model, or whether `litho-generator::Config` remains authoritative and
  `litho-core` becomes a thin public subset.
- [ ] Eliminate duplicated config concepts across `litho-core`, `litho-cli`,
  and `litho-generator` so precedence and defaults are identical from every
  entrypoint.
- [ ] Update README / architecture docs after the CLI and config story is
  actually converged, not before.

Done when:
- [ ] A new user can run the full documentation workflow from `litho` without
  learning a second generation binary.
- [ ] There is one authoritative config story and one authoritative generation
  path.

## P1: Finish QMD Consolidation

Maps to GOALS: 6, 7, 8, 10

### Remove the remaining crate split

- [ ] Merge `litho-qmd-llm` into `litho-qmd-storage`, or explicitly decide not
  to and document a stable two-crate boundary. The current state is transitional
  and still duplicates config-loading concerns.
- [ ] Move `litho-qmd-core` tests/benches to the final owning crate or reduce
  them to a deliberately temporary compatibility suite.
- [ ] Delete `litho-qmd-core` entirely once `litho-qmd-llm` and any remaining
  internal users stop importing it.
- [ ] Remove duplicate repo-config helpers and types once the final crate
  boundary is chosen.

### Finish the module surgery inside `litho-qmd-storage`

- [ ] Move the remaining backend-heavy logic out of `src/lib.rs`.
- [ ] Create a real backend split (`sqlite_impl.rs` and either
  `postgres_impl.rs` or an explicitly documented alternative) instead of
  keeping most implementation in one 74k-line library file.
- [ ] Keep `service.rs`, `traits.rs`, `model.rs`, and config helpers as narrow
  surfaces instead of letting them drift back into a god module pattern.

### Unify QMD configuration

- [x] Start loading repo-local QMD settings from `litho.toml` (for example
  `[qmd]`) while keeping legacy JSON fallback behavior.
- [ ] Add an explicit compatibility warning path for `qmd.config.json` and
  `config/qmd.config.json`.
- [ ] Add tests that cover precedence across `litho.toml`, compatibility JSON,
  `.env`, and explicit environment variables.
- [ ] Update the CLI, MCP server, and docs so backend selection and model
  settings come from the same config story as the rest of Litho.

Done when:
- [ ] QMD is one coherent library surface plus the CLI and MCP binaries.
- [ ] Users can clone a repo and understand QMD backend/config behavior from
  `litho.toml` alone.

## P2: Modernize Generator Orchestration

Maps to GOALS: 2, 3, 4, 5, 10

### Replace ad hoc concurrency with structured orchestration

- [x] Replace the shared bounded-parallel helper internals with `JoinSet` while
  preserving caller result ordering.
- [x] Remove the direct `join_all` + semaphore fan-out from
  `integrations/knowledge_sync.rs`.
- [ ] Add focused regression coverage for error and cancellation behavior in the
  shared concurrency helper.
- [ ] Finish replacing any remaining bespoke `join_all` / fixed-arity
  orchestration sites with clearer structured-concurrency scheduling.
- [ ] Remove the fixed-arity selective-orchestration branches that enumerate
  combinations of affected agents and replace them with data-driven scheduling.
- [ ] Audit blocking work and keep it behind explicit `spawn_blocking`
  boundaries with clear ownership.

### Revisit the stage model

- [ ] Decide whether the current preprocess-all -> research-all -> compose-all
  boundary remains the desired architecture, or whether the pipeline should move
  to streaming handoffs so downstream work can start earlier.
- [ ] If stage streaming is adopted, define the handoff artifacts explicitly so
  validation, retries, and incremental reruns remain deterministic.
- [ ] Keep the current quality gate and review loop behavior intact while the
  orchestration changes underneath it.

### Keep provider behavior operable under higher parallelism

- [ ] Build explicit rate-limit / retry policy around the multi-agent pipeline
  instead of relying on scattered request caps.
- [ ] Keep startup warmup, local-model preparation, and fallback behavior
  observable enough that concurrency failures are debuggable.

Done when:
- [ ] The orchestration layer no longer depends on helper patterns that hide
  cancellation and failure propagation.
- [ ] Stage boundaries are explicit and testable.

## P3: Make Incremental Regeneration Trustworthy

Maps to GOALS: 4, 5, 9

### Improve the change model

- [ ] Move beyond pure `git diff` + path heuristics toward symbol-, module-, or
  AST-aware change targeting.
- [ ] Reuse the ingestion DAG and manifest data for more precise dependency
  tracking instead of treating `input_files` lists as the only provenance
  source.
- [ ] Preserve enough metadata to explain why a section regenerated.
- [ ] Add fixture coverage for rename, delete, and small-edit cases so the
  change-planning logic can be evolved safely.

### Improve output updates

- [ ] Merge regenerated sections into existing outputs instead of treating whole
  files as the only update unit.
- [ ] Track section-level provenance and hashes so validation and diffing are
  meaningful after partial reruns.
- [ ] Add regression tests that prove a small source edit does not force a
  broad rebuild.
- [ ] Add a diff/report artifact that shows exactly which sections changed in an
  incremental run before attempting a user-facing `litho diff` command.

Done when:
- [ ] Incremental mode is materially more precise than “changed files map to
  agent buckets”.
- [ ] The output layer can explain what changed and why.

## P4: Align Extraction Coverage With Product Claims

Maps to GOALS: 2, 5, 9, 10

### Close the language-support gap

- [ ] Decide the supported-language contract for `litho-extract`.
- [ ] Either add real AST/interface/dependency extraction for the languages that
  discovery already recognizes (for example Java, Go, C, C++), or narrow the
  outward claims until that work exists.
- [ ] Keep generator preprocessing heuristics and extractor capabilities in sync
  so the repo is not “recognized” in more languages than it can actually
  analyze structurally.
- [ ] Add fixture-based extraction tests for every language the project claims
  to support structurally.

### Add smarter extraction only after the contract is clear

- [ ] Add documentation-debt / undocumented-public-API scoring only after the
  supported-language matrix is explicit.
- [ ] Add an AST cache only when the extractor contract and invalidation story
  are stable enough to support it.

Done when:
- [ ] README, extractor behavior, and tests all describe the same language
  support level.

## P5: Capability Growth After The Core Refactor

Maps to GOALS: 4, 6, 7, 9

- [ ] Revisit local embedding/search acceleration (`fastembed-rs`, ANN/index
  alternatives) only after the QMD surface is consolidated.
- [ ] Add `litho diff` only after manifests and section provenance are stable
  enough to produce meaningful comparisons.
- [ ] Add `--watch` regeneration only after incremental correctness is strong
  enough that continuous reruns are trustworthy.
- [ ] Keep PDF and other alternate output formats secondary to the Markdown/HTML
  workflow unless the primary path stays simple.

## Deferred / Research Track

These are real ideas, but they should not compete with the architecture work
above until the core workflow is coherent.

- [ ] PostgreSQL 18 migration validation and long-lived service-mode hardening.
- [ ] Multi-repo orchestration across workspaces or submodules.
- [ ] Two-stage reviewer loops beyond the current bounded review pass.
- [ ] Template-constrained recovery for malformed LLM JSON.
- [ ] Redis or external caching layers.
- [ ] Additional coordinator / function-calling experiments once the provider
  and orchestration surfaces are stable.
