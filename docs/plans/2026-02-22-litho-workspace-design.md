# Litho Workspace Design

**Date**: 2026-02-22
**Status**: Approved
**Author**: Claude Opus 4.6 + David Martel

## Problem Statement

deepwiki-rs (Litho) is a Rust-based AI documentation generator that:

1. **Falls back to unreachable Chinese servers**: `LLMConfig::default()` in `config.rs:644` hardcodes `api_base_url = "https://api-inference.modelscope.cn/v1"` with Qwen models. Networks blocking China routing get zero functionality from defaults.

2. **Uses LLM for work AST tools do better**: 6,889 lines of regex-based language processors (`extractors/language_processors/`) do string matching (`content.matches("fn ")`) for complexity and interface extraction. The `code_purpose_analyze.rs` sends every unclassified file to an LLM. Both are jobs for tree-sitter.

3. **Makes 14+ LLM round-trips per codebase**: 8 research agents + 6 composition agents, each a separate inference call. A single codex-cli invocation with pre-extracted context can replace all of them.

4. **Lives in two separate repos**: deepwiki-rs (github.com/David-Martel/deepwiki-rs, upstream sopaco/deepwiki-rs) and litho-book (github.com/David-Martel/litho-book, upstream sopaco/litho-book) share no code but should.

## Architecture: AST-First, LLM-Enhanced

Three layers, each independently useful:

```
Layer 1: litho-extract (always available, no LLM, <5s)
    tree-sitter AST parsing → structured JSON
         │
         ▼
Layer 2: litho-codex (on-demand, codex-cli)
    Single codex exec invocation → C4 architecture docs
         │
         ▼
Layer 3: litho-generator (legacy, rig-based)
    Existing Ollama/OpenAI pipeline (preserved, not default)
```

### Layer 1: `litho-extract` — Tree-Sitter Code Extraction

Replaces the 13 regex language processors and LLM code-purpose classification with in-process tree-sitter AST parsing.

**Input**: Project root path + config (excluded dirs, max depth, extensions)

**Pipeline**:
1. **File discovery** — `ignore` crate (gitignore-aware walkdir)
2. **AST parsing** — tree-sitter with per-language grammars
3. **Interface extraction** — Query AST for functions, structs, classes, imports
4. **Complexity computation** — Cyclomatic complexity from AST node counts (if/for/match/while)
5. **Classification** — Path heuristics + AST patterns (main → entry_point, *_test → test, mod.rs → module)
6. **Dependency graph** — Build from extracted imports/use statements

**Output**: `ExtractedCodebase` (serializable JSON):
```rust
pub struct ExtractedCodebase {
    pub project_name: String,
    pub files: Vec<ExtractedFile>,
    pub dependency_graph: HashMap<String, Vec<String>>,
    pub module_tree: Vec<ModuleNode>,
    pub statistics: ProjectStats,
}

pub struct ExtractedFile {
    pub path: PathBuf,
    pub language: Language,
    pub classification: FileClassification,
    pub complexity: ComplexityMetrics,
    pub dependencies: Vec<Dependency>,
    pub interfaces: Vec<Interface>,
    pub important_lines: Vec<(usize, String)>,
}
```

**Tree-sitter grammars** (Cargo dependencies):
- `tree-sitter-rust`, `tree-sitter-typescript`, `tree-sitter-python`
- `tree-sitter-c-sharp`, `tree-sitter-javascript`
- PowerShell: may need custom grammar or fallback to regex

**ast-grep rules** (`rules/extract/`) remain available for supplementary linting but are not the primary extraction mechanism.

### Layer 2: `litho-codex` — Codex-CLI Provider

Replaces 14 separate LLM round-trips with a single codex-cli invocation.

**Provider trait**:
```rust
#[async_trait]
pub trait DocGenerator: Send + Sync {
    async fn generate(
        &self,
        extracted: &ExtractedCodebase,
        config: &DocConfig,
    ) -> Result<Vec<DocumentSection>>;
}
```

**Implementations**:
1. `CodexExecGenerator` — shells out to `codex exec --full-auto -C <dir> -o <output>`
2. `CodexMcpGenerator` — connects to `codex mcp-server` via stdio (future)
3. `RigGenerator` — legacy path using existing rig-based Ollama/OpenAI pipeline

**Invocation flow (exec mode)**:
1. Serialize `ExtractedCodebase` to JSON
2. Build prompt from templates (`prompts/architecture.md`, etc.)
3. Invoke `codex exec --full-auto -C <project> -o output.md -m gpt-5.3-codex`
4. Parse output into `Vec<DocumentSection>`

**Prompt templates** include the AST-extracted data so codex focuses on analysis, not parsing. Codex can read specific files when deeper investigation is needed (it has file-reading tools built in).

### Layer 3: Legacy Rig Pipeline (Preserved)

The existing 4-stage pipeline (preprocess → research → compose → output) with 8 providers (OpenAI, Ollama, Anthropic, etc.) is preserved as `RigGenerator`. This allows:
- Users who prefer local Ollama inference
- Upstream compatibility with sopaco/deepwiki-rs
- Gradual migration path

**Critical fix**: Change `config.rs:644` default `api_base_url` from `"https://api-inference.modelscope.cn/v1"` to `""` with a clear error: "No LLM API endpoint configured. Set api_base_url in litho.toml or use --provider codex."

## Monorepo Structure

```
/c/codedev/litho-workspace/
├── Cargo.toml                    # [workspace] members = ["crates/*"]
├── crates/
│   ├── litho-core/               # Config, types, cache, i18n (from deepwiki-rs)
│   │   └── src/
│   │       ├── config.rs         # LithoConfig (modelscope.cn defaults removed)
│   │       ├── types/            # Shared types (code, structure, docs)
│   │       ├── cache/            # Cache system
│   │       └── i18n.rs           # Internationalization
│   │
│   ├── litho-extract/            # NEW: tree-sitter AST extraction
│   │   └── src/
│   │       ├── lib.rs            # pub extract(config) → ExtractedCodebase
│   │       ├── discovery.rs      # File discovery (ignore crate)
│   │       ├── parser.rs         # tree-sitter dispatcher
│   │       ├── extractors/       # Per-language query modules
│   │       ├── classify.rs       # Path + AST classification
│   │       ├── complexity.rs     # Cyclomatic complexity
│   │       ├── graph.rs          # Dependency graph
│   │       └── types.rs          # ExtractedCodebase, ExtractedFile
│   │
│   ├── litho-codex/              # NEW: codex-cli provider
│   │   ├── src/
│   │   │   ├── lib.rs            # DocGenerator trait
│   │   │   ├── exec.rs           # codex exec implementation
│   │   │   ├── mcp.rs            # codex mcp-server (future)
│   │   │   └── prompts.rs        # Prompt template loader
│   │   └── prompts/              # Markdown prompt templates
│   │
│   ├── litho-generator/          # Pipeline orchestration (from deepwiki-rs)
│   │   └── src/
│   │       ├── workflow.rs       # Pipeline: extract → generate → output
│   │       ├── preprocess/       # Modified: delegates to litho-extract
│   │       ├── research/         # Preserved: rig-based agents
│   │       ├── compose/          # Preserved: rig-based agents
│   │       └── outlet/           # Output writers (disk, summary, mermaid fixer)
│   │
│   ├── litho-book/               # Web UI reader (from litho-book repo)
│   │   └── src/
│   │       ├── main.rs
│   │       ├── server.rs
│   │       └── filesystem.rs
│   │
│   └── litho-cli/                # CLI binary
│       └── src/
│           └── main.rs           # Subcommands: extract, generate, serve
│
├── rules/                        # ast-grep rules (linting, not extraction)
├── prompts/                      # Codex prompt templates (symlinked into litho-codex)
├── docs/
│   └── plans/                    # Design documents
└── .github/workflows/
    ├── ci.yml                    # Build + test all crates
    └── release.yml               # Binary releases
```

## CLI Interface

```bash
# Layer 1: Fast extraction only (no LLM)
litho extract /path/to/project --json > codebase.json
litho extract /path/to/project --format summary  # Human-readable

# Layer 2: Full documentation via codex
litho generate /path/to/project --provider codex
litho generate /path/to/project --provider codex --sections architecture,workflow

# Layer 3: Legacy rig pipeline
litho generate /path/to/project --provider ollama --model qwen2.5-coder:7b
litho generate /path/to/project --provider openai --api-key $KEY

# Web UI
litho serve ./litho.docs --port 3000
```

## PC_AI Integration

### Doc Pipeline (`Invoke-DocPipeline.ps1`)

Two new pipeline steps:

```powershell
# Step: LithoExtraction (fast, always runs)
function Invoke-LithoExtraction {
    $litho = Get-Command litho-cli -ErrorAction SilentlyContinue
    if (-not $litho) { $litho = "$env:USERPROFILE\bin\litho-cli.exe" }

    & $litho extract . --json | Out-File Reports/LITHO_EXTRACT.json
    & $litho extract . --format summary | Out-File Reports/LITHO_EXTRACT_SUMMARY.md
}

# Step: LithoDocGeneration (optional, requires codex or Ollama)
function Invoke-LithoDocGeneration {
    param([string]$Provider = 'codex')

    & litho-cli generate . --provider $Provider --output docs/auto/litho/
}
```

### CI/CD Integration

PC_AI's `.github/workflows/docs-pipeline.yml`:
- Add litho-cli as a build artifact dependency
- Run `litho extract` on every PR for structural analysis
- Run `litho generate --provider codex` on release branches for full docs

## Error Handling

- **No LLM configured**: Layer 1 (extract) works without any LLM. Layer 2 fails with clear error: "codex-cli not found or not configured."
- **codex exec failure**: Falls back to RigGenerator if configured, otherwise returns extraction-only results.
- **tree-sitter grammar missing**: Files with unsupported languages get basic file-stat extraction (LOC, size) without AST analysis.
- **Network blocked**: No default requests to external services. All external calls require explicit configuration.

## Testing Strategy

- **litho-extract**: Unit tests with fixture repos (small Rust/TS/Python projects). Verify extraction output matches expected JSON.
- **litho-codex**: Integration tests mocking codex exec output. Verify prompt construction and output parsing.
- **litho-generator**: Existing test suite preserved, modified to use litho-extract output.
- **End-to-end**: Run `litho generate` against PC_AI repo and validate output structure.

## Migration Path

1. Create workspace, move existing code into crates
2. Build `litho-extract` with tree-sitter
3. Build `litho-codex` with exec provider
4. Wire into `litho-generator` workflow
5. Fix modelscope.cn defaults
6. Add PC_AI pipeline integration
7. Validate against PC_AI codebase
8. Update CI/CD workflows
