# Litho Workspace Goals

Status: living target-state requirements

This document describes the end-state Litho is working toward. `TODO.md` is the
implementation backlog that should eventually satisfy these requirements.

## Purpose

Litho is a local-first documentation and code-intelligence workspace for large
repositories. It should be able to:

- inspect a codebase structurally,
- identify where documentation is needed,
- generate and validate high-quality documentation,
- make that documentation searchable and navigable,
- and keep the result current as the codebase changes.

## Vision

The final system is a unified toolkit with one primary CLI, a multi-stage
generation pipeline, a repository-local search/index layer, and a human-facing
documentation viewer. It should work on small projects and scale to large
repositories without requiring fragile manual setup.

## Requirements

### 1. Single user-facing entrypoint

- `litho` MUST be the primary command for extraction, generation, search,
  serving, validation, and indexing workflows.
- `litho-generator` MUST remain the orchestration engine for full documentation
  generation.
- `litho-book` MUST serve generated documentation for human review.
- `litho-qmd-cli` and `litho-qmd-mcp` MUST expose the retrieval and agent-access
  layer behind the CLI and MCP interfaces.

Acceptance criteria:

- A new user can discover the main workflow from `litho --help`.
- Common tasks do not require knowing internal crate names.
- The CLI surface should not present duplicate or contradictory commands.

### 2. Repo-aware documentation generation

- Litho MUST understand a repository structurally before generating docs.
- The preprocessing stage MUST extract code structure, relationships, and
  repository metadata from source files.
- The generation pipeline MUST be able to produce documentation sections that
  correspond to the codebase architecture, modules, workflows, and important
  interfaces.
- Documentation output MUST be grounded in the repository contents, not just in
  general model knowledge.

Acceptance criteria:

- Generated docs reference real files and symbols in the repository.
- The pipeline can explain major architectural parts of a project without
  manual prompting for every section.
- Extraction works without requiring an LLM for the structural analysis step.

### 3. Multi-stage orchestration

- The generator MUST retain a staged pipeline for preprocessing, research,
  composition, and validation.
- Each stage MUST have a clear responsibility and handoff boundary.
- The pipeline SHOULD support model routing so simpler work can use cheaper or
  faster models while harder synthesis work can use stronger models.
- The pipeline SHOULD support a review pass before output is finalized.

Acceptance criteria:

- Stage outputs can be traced through the pipeline.
- Research and composition can be changed independently without breaking the
  other stages.
- The system can route different agent roles to different model preferences.

### 4. Quality-gated output

- Generated documentation MUST be validated before it is accepted as final
  output.
- Validation MUST cover at least completeness, accuracy, freshness, grounding,
  coherence, and helpfulness.
- The system MUST be able to reject or re-run low-quality output based on a
  configurable threshold.
- Validation results MUST be visible in the generated artifacts or summary
  output.

Acceptance criteria:

- The generator can report why a document failed validation.
- Quality regressions can be detected across runs.
- A generated doc set is not treated as final if it fails the configured gate.

### 5. Incremental and scalable operation

- Litho MUST support incremental regeneration based on repository changes.
- Incremental runs SHOULD re-execute only the parts of the pipeline that are
  affected by the change.
- The system SHOULD preserve provenance so it can explain why a section was
  regenerated.
- The design SHOULD scale to large repositories by minimizing unnecessary work.

Acceptance criteria:

- A small code change does not force a full end-to-end regeneration.
- The manifest or equivalent metadata can explain what changed since the last
  run.
- The system can fall back to a full run when incremental state is missing or
  invalid.

### 6. Repository-local retrieval and search

- Litho MUST provide a searchable local index for repository artifacts and
  generated documentation.
- Search MUST support keyword search, semantic/vector search, and hybrid query
  modes.
- The index MUST be usable by both CLI users and agent integrations.
- The default local workflow SHOULD work without requiring external services.

Acceptance criteria:

- A repository can be indexed, searched, and queried from the local machine.
- The same content can be retrieved from CLI and MCP surfaces.
- Search results are usable for follow-up documentation work and not just as a
  raw dump of hits.

### 7. Portable storage backends

- QMD MUST support a repo-local SQLite backend.
- QMD MUST continue to support PostgreSQL for shared or service-based use.
- Backend selection MUST be explicit and predictable.
- Repo-local defaults MUST keep the system easy to clone, run, and share.

Acceptance criteria:

- A repository can be indexed and searched with no PostgreSQL dependency.
- PostgreSQL remains available for users who want centralized storage.
- Backend choice does not change the external CLI or MCP contracts.

### 8. Clear configuration model

- Litho MUST converge on a smaller number of configuration entry points.
- Project-level configuration SHOULD live in a single authoritative file or
  tightly related config hierarchy.
- CLI flags, environment variables, and on-disk config MUST agree on precedence.
- Deprecated config paths SHOULD be migrated with clear compatibility behavior.

Acceptance criteria:

- A user can determine the active settings without inspecting multiple
  unrelated schemas.
- Conflicting config sources resolve deterministically.
- New capabilities can be added without multiplying config files.

### 9. Strong test and regression discipline

- Core behavior MUST be covered by automated tests.
- The repository MUST use snapshot or golden-style tests where output stability
  matters.
- HTTP and provider-facing code SHOULD be testable without live network
  dependencies.
- The workspace SHOULD keep clippy and formatting clean as part of normal
  development.

Acceptance criteria:

- Changes to extraction, generation, or search can be regression-tested locally.
- Provider fallbacks and failure modes can be tested deterministically.
- The test suite grows as the product surface grows.

### 10. Maintainable architecture

- The workspace SHOULD reduce unnecessary crate fragmentation over time.
- Thin wrappers and duplicated logic SHOULD be consolidated where they do not
  add a clear boundary.
- Hand-rolled code SHOULD be replaced where a smaller, clearer dependency or
  shared abstraction is better.
- Public APIs SHOULD stay stable enough that the CLI surface remains coherent.

Acceptance criteria:

- The crate graph gets simpler rather than more fragmented.
- Major workflows do not exist in two parallel implementations unless there is
  a deliberate reason.
- New code follows the existing architecture instead of creating an ad hoc path.

## Definition Of Done

Litho reaches its target state when:

- the unified CLI is the obvious way to use the workspace,
- the generator can produce grounded documentation for large repositories,
- incremental reruns are reliable,
- quality gating prevents bad output from shipping,
- QMD works locally by default and can scale to PostgreSQL when needed,
- and the documentation system is maintainable enough that new features extend
  the architecture instead of bypassing it.

## How TODO.md Should Relate To This File

- Each TODO item SHOULD map to one or more requirements above.
- TODO items are implementation steps, not the end state.
- When a TODO item is completed, it should move the project measurably closer to
  one of these requirements.
- If a TODO item no longer supports a requirement, it should be reconsidered or
  removed.

