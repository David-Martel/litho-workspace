# Litho-Workspace: Remaining Phases Implementation Plan

## Context

Phase 1 (Foundation & Robustness) is COMPLETE with 6 commits: CodexRs fallback, serde hardening (490-line serde_helpers.rs), nextest (368 tests), config flexibility, excluded_dirs fixes, research pipeline resilience.

The end-to-end pipeline was tested successfully against david-t-martel (253 files, 44 source files, 12 markdown docs, 521s). However, the generated docs have **critical quality issues**: hallucinated workflows, wrong user personas, fabricated tech stack, shallow content. These are caused by weak LLM prompts that actively distrust the README and provide no grounding constraints for a 7B model.

This plan addresses Phases 2-4 in priority order: **quality fixes first** (highest impact, lowest effort), then incremental mode (architectural enabler), then CLI/format polish.

---

## Sprint 1: LLM Prompt Quality Fixes (2-3 days)

These are high-impact, low-effort changes to prompt templates and preprocessing that dramatically improve doc quality.

### Task 1.1: Fix README trust signal
**File:** `crates/litho-generator/src/generator/step_forward_agent.rs` line 229
**Change:** Replace:
```
"### Previous README Content (Manually entered information, may not be accurate, for reference only)"
```
With:
```
"### Project README (Authoritative project description from the repository maintainer)"
```
**Why:** The current label tells the 7B model to ignore the most reliable input. This single line causes Issues #2 (wrong users) and #7 (fabricated descriptions).

### Task 1.2: Ingest CLAUDE.md and other project docs
**File:** `crates/litho-generator/src/generator/preprocess/extractors/original_document_extractor.rs`
**Change:** Expand `extract()` to also read `CLAUDE.md`, `CONTRIBUTING.md`, `docs/README.md` if they exist. Merge into OriginalDocument (add a `supplementary_docs: Option<String>` field or append to readme).
**Why:** CLAUDE.md contains structured architecture info that is higher quality than README for code projects.

### Task 1.3: Fix trim_markdown to preserve headings
**File:** `original_document_extractor.rs` line 22
**Change:** Remove the `line.starts_with('#')` skip. Headings like "## Architecture" provide crucial structural context.

### Task 1.4: Extract tech stack from manifests
**File:** `original_document_extractor.rs`
**Change:** Add extraction of dependency names from `Cargo.toml`, `pyproject.toml`, `package.json`, `requirements.txt`. Store as `tech_stack: Option<Vec<String>>` in OriginalDocument. Add formatter in step_forward_agent.rs:
```
### Verified Technology Stack (extracted from manifest files)
- Rust: tokio, clap, serde, ...
- Python: pydantic, python-docx, httpx, ...
```
**Why:** Prevents Issue #3 (fabricated tech stack like "Actix-web, FastAPI").

### Task 1.5: Add grounding constraints to research agents
**Files:**
- `crates/litho-generator/src/generator/research/agents/system_context_researcher.rs` — Add: "Target users MUST be derived from the README. If it's a personal tool, say so. Confidence <3 if only guessing from file names."
- `crates/litho-generator/src/generator/research/agents/workflow_researcher.rs` — Add: "Only describe workflows ACTUALLY IMPLEMENTED as code paths. Do NOT create workflows for utility modules or MCP servers."
- All compose agents in `compose/agents/` — Add closing constraint: "Do NOT mention technologies not listed in the research materials. Write 'Insufficient data' rather than fabricating content."

### Task 1.6: Fix core file detection threshold
**File:** `crates/litho-generator/src/generator/preprocess/extractors/structure_extractor.rs` line 427
**Change:** `file.is_core = score > 0.5;` → `file.is_core = score >= 0.5;`
Add bonus: `if path_str.contains("tools") { score += 0.15; }` (Python projects use `tools/` as main source).
**Why:** Critical files like `tools/cv/cv_pipeline.py` score exactly 0.5 and are excluded by the strict `>` comparison.

### Task 1.7: Improve deep-exploration prompts
**File:** `crates/litho-generator/src/generator/research/agents/key_modules_insight.rs`
**Change:** Replace generic system prompt with structured analysis template: PURPOSE, MECHANISM, DATA FLOW, DEPENDENCIES, ERROR HANDLING — all requiring concrete code references.

### Verification (Sprint 1)
```bash
# Rebuild
cd /c/codedev/litho-workspace && cargo build --release -p litho-generator
# Clear cache and re-run
rm -rf /c/codedev/david-t-martel/.litho/cache
target/release/litho-generator.exe --project-path /c/codedev/david-t-martel --output-path /c/codedev/david-t-martel/docs/auto/litho_docs/
# Compare: check user personas, tech stack, workflows against actual project
```

---

## Sprint 2: Documentation Manifest + Change Detection (3 days)

Foundation for incremental mode. Without a manifest, delta computation is impossible.

### Task 2.1: DocumentationManifest struct
**New file:** `crates/litho-generator/src/integrations/manifest.rs`
**Structs:** `DocumentationManifest { version, generated_at, git_commit, git_branch, project_path, file_hashes: HashMap<PathBuf, String>, modules: HashMap<String, ModuleManifest>, total_generation_time_secs }` + `ModuleManifest { agent_type, output_file, input_files, generated_at, content_hash }`
**Deps:** Add `blake3 = "1"` to litho-generator Cargo.toml for file hashing.
**Storage:** Save to `.litho/manifest.json` after every generation run.

### Task 2.2: Wire manifest into workflow.rs
**File:** `crates/litho-generator/src/generator/workflow.rs`
**Change:** After output stage (line 127), collect file hashes + module info from Memory/DocTree and save manifest. Add `DocumentationManifest::save(&self, path)` and `::load(path)` methods.

### Task 2.3: ChangeDetector module
**New file:** `crates/litho-generator/src/integrations/change_detector.rs`
**Implementation:** `git diff --name-status <manifest.git_commit>..HEAD` via `tokio::process::Command`. Map changed files to affected agents using path-based heuristic (conservative: if >30% changed, re-run all).
**Output:** `ChangeSet { changed_files, added_files, removed_files, affected_agents: HashSet<String> }`

### Task 2.4: Register new modules
**File:** `crates/litho-generator/src/integrations/mod.rs`
**Change:** Add `pub mod manifest; pub mod change_detector;`

### Verification (Sprint 2)
```bash
# Run full generation → manifest.json created
target/release/litho-generator.exe --project-path /c/codedev/david-t-martel ...
cat /c/codedev/david-t-martel/.litho/manifest.json | jq '.version, .git_commit, (.modules | keys)'
# Make a small change, verify ChangeDetector identifies it
```

---

## Sprint 3: Incremental Mode + CLI Expansion (3 days)

### Task 3.1: `--incremental` flag
**File:** `crates/litho-generator/src/cli.rs`
**Change:** Add `--incremental` CLI flag. In `Args::to_config()`, set a new `config.incremental: bool` field.

### Task 3.2: `launch_incremental()` in workflow.rs
**File:** `crates/litho-generator/src/generator/workflow.rs`
**New function:** `pub async fn launch_incremental(c: &Config) -> Result<()>`
**Logic:**
1. Load manifest from `.litho/manifest.json`
2. Run ChangeDetector to get affected agents
3. Run preprocess only for changed files (delta mode)
4. Run only affected research agents
5. Run only affected compose agents
6. Merge new docs with existing output
7. Save updated manifest

**Performance target:** <60s for <10% file changes.

### Task 3.3: litho-cli `status` command
**File:** `crates/litho-cli/src/main.rs`
**Implementation:** Read `.litho/manifest.json`, display: last generation time, file count, staleness, git distance. ~50 LOC.

### Task 3.4: litho-cli `serve` command
**File:** `crates/litho-cli/src/main.rs`
**Implementation:** Shell out to `litho-book` binary with args. ~30 LOC.

### Task 3.5: litho-cli `validate` command (basic)
**File:** `crates/litho-cli/src/main.rs`
**Implementation:** Scan generated docs for: broken file path references (backtick-quoted paths that don't exist on disk), stale references (files renamed/deleted since manifest). ~80 LOC.

### Verification (Sprint 3)
```bash
# Full run → incremental run with small change → verify <60s
time target/release/litho-generator.exe --project-path /c/codedev/david-t-martel ...
echo "# test" >> /c/codedev/david-t-martel/tools/cv/cv_pipeline.py
time target/release/litho-generator.exe --incremental --project-path /c/codedev/david-t-martel ...
# CLI commands
litho status /c/codedev/david-t-martel
litho validate /c/codedev/david-t-martel/docs/auto/litho_docs/
```

---

## Sprint 4: Multi-Format Output + CI (2 days)

### Task 4.1: HtmlOutlet via pulldown-cmark
**New file:** `crates/litho-generator/src/generator/outlet/html_outlet.rs`
**Implementation:** Implement `Outlet` trait. Convert DocTree markdown to HTML via `pulldown-cmark`. Wrap in a minimal HTML template.
**Dep:** Add `pulldown-cmark = "0.12"` to Cargo.toml.

### Task 4.2: `--format` flag
**File:** `crates/litho-generator/src/cli.rs`
**Change:** Add `--format {md,html}` flag. Wire into outlet selection in workflow.rs.

### Task 4.3: GitHub Actions workflow
**New file:** `.github/workflows/docs.yml`
**Content:** On push to main: full generation + validate. On PR: incremental + validate. Upload docs as artifact.

### Task 4.4: Tests for new functionality
- Manifest round-trip serialization
- ChangeDetector with mock git
- CLI subcommand parsing
- HtmlOutlet output format

### Verification (Sprint 4)
```bash
target/release/litho-generator.exe --format html --project-path /c/codedev/david-t-martel ...
# Verify HTML output renders in browser
cargo nextest run --workspace --no-fail-fast
```

---

## Deferred (not in scope)

- **rig-core migration (4.3):** Deeply wired through 604-LOC providers.rs + every agent. Too risky to combine with quality/incremental work. Separate sprint after all above is stable.
- **Phase 2.2 language expansion:** litho-generator already has 13 language processors. Adding tree-sitter extractors for Go/Java/C++ is low priority vs fixing doc quality.
- **Phase 2.1 PatternAnalyzer:** Valuable (detects undocumented APIs, async chains, state machines) but depends on the quality fixes landing first. Separate follow-up.
- **Phase 2.3 AST cache:** Depends on PatternAnalyzer. Separate follow-up.
- **PDF/DOCX output:** Requires pandoc external dependency. Low priority.

---

## Critical Files Summary

| File | Sprint | Change |
|------|--------|--------|
| `generator/step_forward_agent.rs:229` | 1 | Fix README trust label |
| `preprocess/extractors/original_document_extractor.rs` | 1 | Ingest CLAUDE.md, fix trim_markdown, extract tech stack |
| `preprocess/extractors/structure_extractor.rs:427` | 1 | Fix is_core threshold (> to >=), add tools/ bonus |
| `research/agents/system_context_researcher.rs` | 1 | Grounding constraints |
| `research/agents/workflow_researcher.rs` | 1 | Anti-hallucination rules |
| `research/agents/key_modules_insight.rs` | 1 | Structured analysis prompt |
| `compose/agents/*.rs` (6 files) | 1 | Anti-fabrication closing instructions |
| `integrations/manifest.rs` (new) | 2 | DocumentationManifest + BLAKE3 |
| `integrations/change_detector.rs` (new) | 2 | Git diff + file hash delta |
| `generator/workflow.rs` | 2-3 | Save manifest, launch_incremental() |
| `cli.rs` | 3-4 | --incremental, --format flags |
| `litho-cli/src/main.rs` | 3 | status, serve, validate commands |
| `outlet/html_outlet.rs` (new) | 4 | HTML output via pulldown-cmark |
| `.github/workflows/docs.yml` (new) | 4 | CI/CD for doc generation |

## Existing Functions to Reuse

- `CacheManager` — already handles prompt-keyed MD5 caching in `.litho/cache/`
- `Memory` (shared async state) — agent data exchange, scope-based isolation
- `DiskOutlet` / `Outlet` trait — extensible output format pattern
- `SummaryOutlet` — timing stats already collected and stored
- `KnowledgeSyncer` in integrations/ — existing integration module pattern
- `serde_helpers.rs` — 6 public deserializer functions for tolerant LLM parsing
- `OllamaExtractorWrapper` — 5-strategy JSON parsing cascade
