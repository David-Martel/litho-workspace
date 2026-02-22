# Litho Workspace Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Restructure deepwiki-rs + litho-book into a Cargo workspace monorepo with tree-sitter AST extraction and codex-cli doc generation, replacing LLM-heavy preprocessing and China-routed defaults.

**Architecture:** Three-layer system: (1) `litho-extract` for deterministic tree-sitter code analysis, (2) `litho-codex` for codex-cli prose synthesis, (3) preserved rig-based pipeline as legacy fallback. All layers are independent — Layer 1 works without any LLM.

**Tech Stack:** Rust 2024 edition, tree-sitter 0.26 + language grammars, codex-cli exec/mcp-server, clap 4.5, tokio 1.47, serde/serde_json, ignore 0.4

**Scope:** litho-workspace is a standalone tool for any codebase. PC_AI is the validation target but not a dependency.

---

### Task 1: Create Workspace Skeleton

**Files:**
- Create: `/c/codedev/litho-workspace/Cargo.toml`
- Create: `/c/codedev/litho-workspace/crates/litho-core/Cargo.toml`
- Create: `/c/codedev/litho-workspace/crates/litho-core/src/lib.rs`
- Create: `/c/codedev/litho-workspace/crates/litho-extract/Cargo.toml`
- Create: `/c/codedev/litho-workspace/crates/litho-extract/src/lib.rs`
- Create: `/c/codedev/litho-workspace/crates/litho-codex/Cargo.toml`
- Create: `/c/codedev/litho-workspace/crates/litho-codex/src/lib.rs`
- Create: `/c/codedev/litho-workspace/crates/litho-cli/Cargo.toml`
- Create: `/c/codedev/litho-workspace/crates/litho-cli/src/main.rs`

**Step 1: Create workspace root Cargo.toml**

```toml
[workspace]
members = ["crates/*"]
resolver = "3"

[workspace.package]
version = "2.0.0-alpha.1"
edition = "2024"
license = "MIT"
repository = "https://github.com/David-Martel/litho-workspace"

[workspace.dependencies]
# Shared across crates
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
anyhow = "1.0"
thiserror = "2.0"
tokio = { version = "1.47", features = ["full"] }
clap = { version = "4.5", features = ["derive"] }
async-trait = "0.1"
chrono = { version = "0.4", features = ["serde"] }

# tree-sitter grammars
tree-sitter = "0.26"
tree-sitter-rust = "0.24"
tree-sitter-typescript = "0.23"
tree-sitter-python = "0.25"
tree-sitter-c-sharp = "0.23"
tree-sitter-javascript = "0.23"

# File walking
ignore = "0.4"

# Internal crates
litho-core = { path = "crates/litho-core" }
litho-extract = { path = "crates/litho-extract" }
litho-codex = { path = "crates/litho-codex" }
```

**Step 2: Create litho-core Cargo.toml**

```toml
[package]
name = "litho-core"
version.workspace = true
edition.workspace = true

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
anyhow = { workspace = true }
thiserror = { workspace = true }
toml = "0.9"
chrono = { workspace = true }
```

**Step 3: Create litho-core/src/lib.rs stub**

```rust
pub mod config;
pub mod types;

// Re-exports
pub use config::LithoConfig;
```

**Step 4: Create litho-extract Cargo.toml**

```toml
[package]
name = "litho-extract"
version.workspace = true
edition.workspace = true

[dependencies]
litho-core = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
anyhow = { workspace = true }
tree-sitter = { workspace = true }
tree-sitter-rust = { workspace = true }
tree-sitter-typescript = { workspace = true }
tree-sitter-python = { workspace = true }
tree-sitter-c-sharp = { workspace = true }
tree-sitter-javascript = { workspace = true }
ignore = { workspace = true }
```

**Step 5: Create litho-extract/src/lib.rs stub**

```rust
pub mod discovery;
pub mod parser;
pub mod extractors;
pub mod classify;
pub mod complexity;
pub mod graph;
pub mod types;

pub use types::ExtractedCodebase;

pub async fn extract(project_path: &std::path::Path) -> anyhow::Result<ExtractedCodebase> {
    todo!("Task 3 implements this")
}
```

**Step 6: Create litho-codex Cargo.toml**

```toml
[package]
name = "litho-codex"
version.workspace = true
edition.workspace = true

[dependencies]
litho-core = { workspace = true }
litho-extract = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
anyhow = { workspace = true }
tokio = { workspace = true }
async-trait = { workspace = true }
```

**Step 7: Create litho-codex/src/lib.rs stub**

```rust
pub mod provider;
pub mod exec;

pub use provider::DocGenerator;
```

**Step 8: Create litho-cli Cargo.toml and main.rs**

```toml
[package]
name = "litho-cli"
version.workspace = true
edition.workspace = true

[[bin]]
name = "litho"
path = "src/main.rs"

[dependencies]
litho-core = { workspace = true }
litho-extract = { workspace = true }
litho-codex = { workspace = true }
clap = { workspace = true }
tokio = { workspace = true }
serde_json = { workspace = true }
anyhow = { workspace = true }
```

```rust
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "litho", about = "AST-first code documentation generator")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Extract codebase structure (no LLM required)
    Extract {
        #[arg(default_value = ".")]
        project_path: PathBuf,
        #[arg(long, default_value = "json")]
        format: String,
    },
    /// Generate documentation (requires codex-cli or LLM backend)
    Generate {
        #[arg(default_value = ".")]
        project_path: PathBuf,
        #[arg(long, default_value = "codex")]
        provider: String,
        #[arg(long, default_value = "./litho.docs")]
        output: PathBuf,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Extract { project_path, format } => {
            let extracted = litho_extract::extract(&project_path).await?;
            match format.as_str() {
                "json" => println!("{}", serde_json::to_string_pretty(&extracted)?),
                _ => println!("{}", extracted.summary()),
            }
        }
        Commands::Generate { project_path, provider, output } => {
            let extracted = litho_extract::extract(&project_path).await?;
            let docs = match provider.as_str() {
                "codex" => litho_codex::exec::generate(&extracted, &output).await?,
                _ => anyhow::bail!("Unknown provider: {provider}. Use 'codex' or configure litho.toml for rig-based backends."),
            };
            println!("Documentation generated: {}", output.display());
            let _ = docs;
        }
    }
    Ok(())
}
```

**Step 9: Verify workspace compiles**

Run: `cd /c/codedev/litho-workspace && cargo check 2>&1`
Expected: Compile succeeds (stub code with todo!() is valid)

**Step 10: Commit**

```bash
cd /c/codedev/litho-workspace
git add -A
git commit -m "feat: scaffold litho-workspace Cargo workspace

Workspace with 4 crates: litho-core, litho-extract, litho-codex, litho-cli.
Stubs only — implementation follows in subsequent tasks."
```

---

### Task 2: Implement litho-core (Config + Types)

**Files:**
- Create: `crates/litho-core/src/config.rs`
- Create: `crates/litho-core/src/types.rs`
- Test: `crates/litho-core/src/config.rs` (inline #[cfg(test)])

**Step 1: Write config test**

```rust
// crates/litho-core/src/config.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_no_external_urls() {
        let config = LithoConfig::default();
        assert!(config.llm.api_base_url.is_empty(),
            "Default must not point to external servers");
    }

    #[test]
    fn config_roundtrip_toml() {
        let config = LithoConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: LithoConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.llm.provider, config.llm.provider);
    }

    #[test]
    fn config_from_file_overrides_defaults() {
        let toml = r#"
[llm]
provider = "ollama"
api_base_url = "http://localhost:11434"
model_efficient = "qwen2.5-coder:3b"
"#;
        let config: LithoConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.llm.api_base_url, "http://localhost:11434");
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cd /c/codedev/litho-workspace && cargo test -p litho-core 2>&1`
Expected: FAIL (LithoConfig not defined)

**Step 3: Implement config.rs**

Port from `deepwiki-rs/src/config.rs` with these changes:
- `api_base_url` default: `""` (empty, not modelscope.cn)
- `model_efficient` default: `""` (empty, not Qwen)
- `model_powerful` default: `""` (empty, not Qwen)
- `max_tokens` default: `8192` (not 131072)
- Add `provider: DocProvider` enum: `Codex`, `Ollama`, `OpenAI`, `Anthropic`

```rust
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct LithoConfig {
    pub project_path: PathBuf,
    pub output_path: PathBuf,
    pub internal_path: PathBuf,
    pub target_language: String,
    pub max_depth: u8,
    pub max_file_size: u64,
    pub excluded_dirs: Vec<String>,
    pub excluded_extensions: Vec<String>,
    pub llm: LlmConfig,
    pub cache: CacheConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum LlmProvider {
    Codex,
    Ollama,
    OpenAI,
    Anthropic,
    Gemini,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct LlmConfig {
    pub provider: LlmProvider,
    pub api_key: String,
    pub api_base_url: String,
    pub model_efficient: String,
    pub model_powerful: String,
    pub max_tokens: u32,
    pub temperature: Option<f64>,
    pub timeout_seconds: u64,
    pub disable_preset_tools: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct CacheConfig {
    pub enabled: bool,
    pub cache_dir: PathBuf,
    pub expire_hours: u64,
}

impl Default for LithoConfig {
    fn default() -> Self {
        Self {
            project_path: PathBuf::from("."),
            output_path: PathBuf::from("./litho.docs"),
            internal_path: PathBuf::from(".litho"),
            target_language: "en".into(),
            max_depth: 10,
            max_file_size: 65536,
            excluded_dirs: vec![
                ".git", "target", "node_modules", ".litho", "__pycache__",
                "dist", "build", ".next", "vendor",
            ].into_iter().map(String::from).collect(),
            excluded_extensions: vec![
                "jpg", "png", "gif", "pdf", "exe", "dll", "so", "dylib",
                "wasm", "lock", "sum",
            ].into_iter().map(String::from).collect(),
            llm: LlmConfig::default(),
            cache: CacheConfig::default(),
        }
    }
}

impl Default for LlmProvider {
    fn default() -> Self { Self::Codex }
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: LlmProvider::default(),
            api_key: String::new(),
            api_base_url: String::new(), // NO external default
            model_efficient: String::new(),
            model_powerful: String::new(),
            max_tokens: 8192,
            temperature: Some(0.1),
            timeout_seconds: 300,
            disable_preset_tools: true,
        }
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            cache_dir: PathBuf::from(".litho/cache"),
            expire_hours: 720,
        }
    }
}

impl LithoConfig {
    pub fn from_file(path: &std::path::Path) -> anyhow::Result<Self> {
        let mut content = String::new();
        std::fs::File::open(path)?.read_to_string(&mut content)?;
        Ok(toml::from_str(&content)?)
    }
}
```

**Step 4: Implement types.rs** (shared types for extraction output)

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Rust, TypeScript, JavaScript, Python, CSharp, PowerShell,
    Java, Go, Ruby, Cpp, C, Unknown,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FileClassification {
    EntryPoint, Library, Module, Test, Config, Documentation,
    BuildScript, Migration, Unknown,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ComplexityMetrics {
    pub cyclomatic: f64,
    pub lines_of_code: usize,
    pub functions: usize,
    pub classes: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Dependency {
    pub source: String,
    pub target: String,
    pub kind: String,  // "use", "import", "include", "require"
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Interface {
    pub name: String,
    pub kind: String,       // "function", "struct", "class", "trait", "enum"
    pub visibility: String, // "pub", "pub(crate)", "private"
    pub signature: String,  // One-line signature
    pub line: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExtractedFile {
    pub path: PathBuf,
    pub language: Language,
    pub classification: FileClassification,
    pub complexity: ComplexityMetrics,
    pub dependencies: Vec<Dependency>,
    pub interfaces: Vec<Interface>,
    pub lines_of_code: usize,
    pub size_bytes: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProjectStats {
    pub total_files: usize,
    pub total_loc: usize,
    pub languages: HashMap<String, usize>,  // language → file count
    pub top_complex_files: Vec<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExtractedCodebase {
    pub project_name: String,
    pub files: Vec<ExtractedFile>,
    pub dependency_graph: HashMap<String, Vec<String>>,
    pub statistics: ProjectStats,
}

impl ExtractedCodebase {
    pub fn summary(&self) -> String {
        format!(
            "{}: {} files, {} LOC, {} languages",
            self.project_name,
            self.statistics.total_files,
            self.statistics.total_loc,
            self.statistics.languages.len(),
        )
    }
}
```

**Step 5: Run tests**

Run: `cd /c/codedev/litho-workspace && cargo test -p litho-core 2>&1`
Expected: 3 tests PASS

**Step 6: Commit**

```bash
git add -A
git commit -m "feat(litho-core): config with safe defaults + shared types

No external API defaults. Empty api_base_url forces explicit configuration.
Types: ExtractedCodebase, ExtractedFile, Interface, Dependency, etc."
```

---

### Task 3: Implement litho-extract (Tree-Sitter Extraction)

**Files:**
- Create: `crates/litho-extract/src/discovery.rs`
- Create: `crates/litho-extract/src/parser.rs`
- Create: `crates/litho-extract/src/extractors/mod.rs`
- Create: `crates/litho-extract/src/extractors/rust.rs`
- Create: `crates/litho-extract/src/extractors/typescript.rs`
- Create: `crates/litho-extract/src/extractors/python.rs`
- Create: `crates/litho-extract/src/extractors/csharp.rs`
- Create: `crates/litho-extract/src/classify.rs`
- Create: `crates/litho-extract/src/complexity.rs`
- Create: `crates/litho-extract/src/graph.rs`
- Create: `crates/litho-extract/src/types.rs` (re-export from core)
- Modify: `crates/litho-extract/src/lib.rs`
- Test: `crates/litho-extract/tests/extract_rust.rs`

**Step 1: Write integration test with a fixture Rust file**

```rust
// crates/litho-extract/tests/extract_rust.rs
use litho_extract::extractors::rust::RustExtractor;
use litho_extract::extractors::Extractor;
use std::path::Path;

#[test]
fn extracts_rust_pub_functions() {
    let code = r#"
pub fn hello(name: &str) -> String {
    format!("Hello, {name}!")
}

fn private_helper() {}

pub struct Config {
    pub path: String,
    port: u16,
}

pub trait Handler: Send + Sync {
    fn handle(&self, req: Request) -> Response;
}
"#;
    let extractor = RustExtractor::new();
    let interfaces = extractor.extract_interfaces(code, Path::new("src/lib.rs"));

    assert_eq!(interfaces.len(), 4); // hello, private_helper, Config, Handler
    let pub_fns: Vec<_> = interfaces.iter()
        .filter(|i| i.visibility == "pub" && i.kind == "function")
        .collect();
    assert_eq!(pub_fns.len(), 1);
    assert_eq!(pub_fns[0].name, "hello");
}

#[test]
fn extracts_rust_use_dependencies() {
    let code = r#"
use std::path::PathBuf;
use crate::config::LithoConfig;
use super::types::ExtractedFile;
"#;
    let extractor = RustExtractor::new();
    let deps = extractor.extract_dependencies(code, Path::new("src/main.rs"));
    assert_eq!(deps.len(), 3);
    assert!(deps.iter().any(|d| d.target == "std::path::PathBuf"));
}
```

**Step 2: Run test to verify it fails**

Run: `cd /c/codedev/litho-workspace && cargo test -p litho-extract 2>&1`
Expected: FAIL (RustExtractor not defined)

**Step 3: Implement discovery.rs**

```rust
use ignore::WalkBuilder;
use litho_core::types::Language;
use std::path::{Path, PathBuf};

pub struct DiscoveredFile {
    pub path: PathBuf,
    pub language: Language,
    pub size: u64,
}

pub fn discover_files(
    root: &Path,
    excluded_dirs: &[String],
    excluded_extensions: &[String],
    max_file_size: u64,
) -> Vec<DiscoveredFile> {
    let mut files = Vec::new();

    let walker = WalkBuilder::new(root)
        .hidden(true)       // skip hidden by default
        .git_ignore(true)   // respect .gitignore
        .build();

    for entry in walker.flatten() {
        let path = entry.path();
        if !path.is_file() { continue; }

        // Check excluded dirs
        if excluded_dirs.iter().any(|d| path.components().any(|c|
            c.as_os_str().to_str().map(|s| s == d).unwrap_or(false)
        )) { continue; }

        // Check excluded extensions
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if excluded_extensions.iter().any(|e| e == ext) { continue; }
        }

        // Check file size
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if size > max_file_size || size == 0 { continue; }

        let language = detect_language(path);
        files.push(DiscoveredFile { path: path.to_owned(), language, size });
    }

    files
}

pub fn detect_language(path: &Path) -> Language {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => Language::Rust,
        Some("ts" | "tsx") => Language::TypeScript,
        Some("js" | "jsx" | "mjs") => Language::JavaScript,
        Some("py") => Language::Python,
        Some("cs") => Language::CSharp,
        Some("ps1" | "psm1" | "psd1") => Language::PowerShell,
        Some("java") => Language::Java,
        Some("go") => Language::Go,
        Some("rb") => Language::Ruby,
        Some("cpp" | "cc" | "cxx" | "hpp") => Language::Cpp,
        Some("c" | "h") => Language::C,
        _ => Language::Unknown,
    }
}
```

**Step 4: Implement Extractor trait and RustExtractor**

```rust
// crates/litho-extract/src/extractors/mod.rs
pub mod rust;
pub mod typescript;
pub mod python;
pub mod csharp;

use litho_core::types::{Dependency, Interface};
use std::path::Path;

pub trait Extractor: Send + Sync {
    fn extract_interfaces(&self, content: &str, path: &Path) -> Vec<Interface>;
    fn extract_dependencies(&self, content: &str, path: &Path) -> Vec<Dependency>;
}
```

```rust
// crates/litho-extract/src/extractors/rust.rs
use super::Extractor;
use litho_core::types::{Dependency, Interface};
use std::path::Path;

pub struct RustExtractor {
    parser: tree_sitter::Parser,
}

impl RustExtractor {
    pub fn new() -> Self {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into())
            .expect("Failed to load Rust grammar");
        Self { parser }
    }
}

impl Extractor for RustExtractor {
    fn extract_interfaces(&self, content: &str, _path: &Path) -> Vec<Interface> {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).unwrap();

        let tree = match parser.parse(content, None) {
            Some(t) => t,
            None => return vec![],
        };

        let mut interfaces = Vec::new();
        let mut cursor = tree.walk();

        visit_node(&mut cursor, content, &mut interfaces);
        interfaces
    }

    fn extract_dependencies(&self, content: &str, _path: &Path) -> Vec<Dependency> {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).unwrap();

        let tree = match parser.parse(content, None) {
            Some(t) => t,
            None => return vec![],
        };

        let mut deps = Vec::new();
        let source = content.as_bytes();

        // Walk AST looking for use_declaration nodes
        let mut cursor = tree.walk();
        walk_for_uses(&mut cursor, source, &mut deps);
        deps
    }
}

fn visit_node(
    cursor: &mut tree_sitter::TreeCursor,
    source: &str,
    interfaces: &mut Vec<Interface>,
) {
    let node = cursor.node();
    let kind = node.kind();

    match kind {
        "function_item" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = &source[name_node.byte_range()];
                let vis = get_visibility(&node, source);
                let line = node.start_position().row + 1;
                let sig = &source[node.byte_range()];
                let sig_line = sig.lines().next().unwrap_or(sig);

                interfaces.push(Interface {
                    name: name.to_string(),
                    kind: "function".into(),
                    visibility: vis,
                    signature: sig_line.trim().to_string(),
                    line,
                });
            }
        }
        "struct_item" => extract_type_def(cursor, source, interfaces, "struct"),
        "enum_item" => extract_type_def(cursor, source, interfaces, "enum"),
        "trait_item" => extract_type_def(cursor, source, interfaces, "trait"),
        "impl_item" => { /* skip impl blocks, methods captured via fn */ }
        _ => {}
    }

    if cursor.goto_first_child() {
        loop {
            visit_node(cursor, source, interfaces);
            if !cursor.goto_next_sibling() { break; }
        }
        cursor.goto_parent();
    }
}

fn extract_type_def(
    cursor: &mut tree_sitter::TreeCursor,
    source: &str,
    interfaces: &mut Vec<Interface>,
    kind: &str,
) {
    let node = cursor.node();
    if let Some(name_node) = node.child_by_field_name("name") {
        let name = &source[name_node.byte_range()];
        let vis = get_visibility(&node, source);
        let line = node.start_position().row + 1;
        interfaces.push(Interface {
            name: name.to_string(),
            kind: kind.into(),
            visibility: vis,
            signature: format!("{vis} {kind} {name}",
                vis = if vis == "pub" { "pub" } else { "" },
                kind = kind, name = name).trim().to_string(),
            line,
        });
    }
}

fn get_visibility(node: &tree_sitter::Node, source: &str) -> String {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if child.kind() == "visibility_modifier" {
                return source[child.byte_range()].to_string();
            }
        }
    }
    "private".into()
}

fn walk_for_uses(
    cursor: &mut tree_sitter::TreeCursor,
    source: &[u8],
    deps: &mut Vec<Dependency>,
) {
    let node = cursor.node();
    if node.kind() == "use_declaration" {
        let text = std::str::from_utf8(&source[node.byte_range()]).unwrap_or("");
        // Extract the path after "use "
        if let Some(path) = text.strip_prefix("use ").and_then(|s| s.strip_suffix(';')) {
            deps.push(Dependency {
                source: String::new(),
                target: path.trim().to_string(),
                kind: "use".into(),
            });
        }
    }

    if cursor.goto_first_child() {
        loop {
            walk_for_uses(cursor, source, deps);
            if !cursor.goto_next_sibling() { break; }
        }
        cursor.goto_parent();
    }
}
```

**Step 5: Create minimal TypeScript, Python, C# extractors** (same pattern, different grammars)

Each follows the same structure as RustExtractor but with language-specific node kinds:
- TypeScript: `function_declaration`, `class_declaration`, `interface_declaration`, `import_statement`
- Python: `function_definition`, `class_definition`, `import_statement`, `import_from_statement`
- C#: `method_declaration`, `class_declaration`, `interface_declaration`, `using_directive`

**Step 6: Implement classify.rs** (path heuristics + AST pattern classification)

```rust
use litho_core::types::FileClassification;
use std::path::Path;

pub fn classify_file(path: &Path, has_main: bool) -> FileClassification {
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    let path_str = path.to_string_lossy().to_lowercase();

    // Test files
    if name.ends_with("_test") || name.ends_with(".test")
        || name.starts_with("test_") || path_str.contains("/tests/")
        || path_str.contains("\\tests\\") || ext == "spec.ts"
    {
        return FileClassification::Test;
    }

    // Entry points
    if has_main || name == "main" || name == "program" || name == "app" {
        return FileClassification::EntryPoint;
    }

    // Config
    if matches!(ext, "toml" | "json" | "yaml" | "yml" | "ini" | "env")
        || name.contains("config") || name.contains("settings")
    {
        return FileClassification::Config;
    }

    // Build scripts
    if name == "build" || name == "Makefile" || name == "Cargo"
        || ext == "cmake" || name.ends_with(".ps1") && path_str.contains("build")
    {
        return FileClassification::BuildScript;
    }

    // Documentation
    if matches!(ext, "md" | "rst" | "txt" | "adoc") {
        return FileClassification::Documentation;
    }

    // Module index files
    if name == "mod" || name == "index" || name == "lib"
        || name == "__init__"
    {
        return FileClassification::Module;
    }

    // Library code (default for source files)
    if matches!(ext, "rs" | "ts" | "js" | "py" | "cs" | "java" | "go" | "rb") {
        return FileClassification::Library;
    }

    FileClassification::Unknown
}
```

**Step 7: Implement complexity.rs**

Simple cyclomatic complexity from tree-sitter node counts.

**Step 8: Implement graph.rs**

Build dependency graph from extracted dependencies.

**Step 9: Wire into lib.rs extract() function**

Combine discovery → parse → extract → classify → graph into the public API.

**Step 10: Run tests**

Run: `cd /c/codedev/litho-workspace && cargo test -p litho-extract 2>&1`
Expected: All tests PASS

**Step 11: Commit**

```bash
git add -A
git commit -m "feat(litho-extract): tree-sitter AST extraction for Rust/TS/Python/C#

Replaces regex-based language processors with proper AST parsing.
Extracts: functions, structs, classes, traits, imports, complexity.
No LLM required — deterministic, reproducible, <5s for typical repos."
```

---

### Task 4: Implement litho-codex (Codex-CLI Provider)

**Files:**
- Create: `crates/litho-codex/src/provider.rs`
- Create: `crates/litho-codex/src/exec.rs`
- Create: `crates/litho-codex/src/prompts.rs`
- Create: `prompts/architecture.md`
- Create: `prompts/overview.md`
- Modify: `crates/litho-codex/src/lib.rs`
- Test: `crates/litho-codex/tests/prompt_test.rs`

**Step 1: Write prompt construction test**

```rust
// crates/litho-codex/tests/prompt_test.rs
use litho_codex::prompts::build_prompt;
use litho_core::types::ExtractedCodebase;

#[test]
fn prompt_includes_project_stats() {
    let codebase = ExtractedCodebase {
        project_name: "test-project".into(),
        files: vec![],
        dependency_graph: Default::default(),
        statistics: litho_core::types::ProjectStats {
            total_files: 42,
            total_loc: 5000,
            languages: [("Rust".into(), 30), ("Python".into(), 12)].into(),
            top_complex_files: vec![],
        },
    };
    let prompt = build_prompt(&codebase, "architecture");
    assert!(prompt.contains("test-project"));
    assert!(prompt.contains("42 files"));
    assert!(prompt.contains("Rust"));
}
```

**Step 2: Run test to verify it fails**

Run: `cd /c/codedev/litho-workspace && cargo test -p litho-codex 2>&1`
Expected: FAIL

**Step 3: Implement provider trait**

```rust
// crates/litho-codex/src/provider.rs
use async_trait::async_trait;
use litho_core::types::ExtractedCodebase;
use std::path::Path;

pub struct DocumentSection {
    pub title: String,
    pub content: String,
    pub filename: String,
}

#[async_trait]
pub trait DocGenerator: Send + Sync {
    async fn generate(
        &self,
        extracted: &ExtractedCodebase,
        project_path: &Path,
        output_path: &Path,
    ) -> anyhow::Result<Vec<DocumentSection>>;
}
```

**Step 4: Implement exec.rs** (codex exec subprocess)

```rust
// crates/litho-codex/src/exec.rs
use crate::prompts;
use crate::provider::{DocGenerator, DocumentSection};
use async_trait::async_trait;
use litho_core::types::ExtractedCodebase;
use std::path::Path;
use std::process::Command;

pub struct CodexExecGenerator {
    pub model: String,
    pub sandbox: String,
}

impl Default for CodexExecGenerator {
    fn default() -> Self {
        Self {
            model: String::new(), // Use codex default
            sandbox: "read-only".into(),
        }
    }
}

#[async_trait]
impl DocGenerator for CodexExecGenerator {
    async fn generate(
        &self,
        extracted: &ExtractedCodebase,
        project_path: &Path,
        output_path: &Path,
    ) -> anyhow::Result<Vec<DocumentSection>> {
        let prompt = prompts::build_prompt(extracted, "full");

        let mut cmd = Command::new("codex");
        cmd.arg("exec")
            .arg("--full-auto")
            .arg("-C").arg(project_path)
            .arg("-s").arg(&self.sandbox)
            .arg("-o").arg(output_path.join("codex-output.md"));

        if !self.model.is_empty() {
            cmd.arg("-m").arg(&self.model);
        }

        cmd.arg(&prompt);

        let output = cmd.output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("codex exec failed: {stderr}");
        }

        // Read the output file
        let content = std::fs::read_to_string(output_path.join("codex-output.md"))
            .unwrap_or_else(|_| String::from_utf8_lossy(&output.stdout).to_string());

        Ok(vec![DocumentSection {
            title: "Architecture Documentation".into(),
            content,
            filename: "architecture.md".into(),
        }])
    }
}

/// Convenience function for CLI
pub async fn generate(
    extracted: &ExtractedCodebase,
    output_path: &Path,
) -> anyhow::Result<Vec<DocumentSection>> {
    let gen = CodexExecGenerator::default();
    let project_path = Path::new(".");
    gen.generate(extracted, project_path, output_path).await
}
```

**Step 5: Implement prompts.rs**

```rust
use litho_core::types::ExtractedCodebase;

pub fn build_prompt(codebase: &ExtractedCodebase, section: &str) -> String {
    let stats = &codebase.statistics;
    let lang_summary: Vec<String> = stats.languages.iter()
        .map(|(lang, count)| format!("{lang}: {count} files"))
        .collect();

    let top_files: Vec<String> = codebase.files.iter()
        .take(20)
        .map(|f| format!("  - {} ({:?}, {} LOC, complexity={:.0})",
            f.path.display(), f.classification, f.lines_of_code, f.complexity.cyclomatic))
        .collect();

    let interfaces: Vec<String> = codebase.files.iter()
        .flat_map(|f| f.interfaces.iter().filter(|i| i.visibility == "pub")
            .map(move |i| format!("  - {}:{} {} {} {}",
                f.path.display(), i.line, i.kind, i.name, i.signature)))
        .take(100)
        .collect();

    format!(r#"You are analyzing the "{name}" codebase to produce C4-model architecture documentation.

## Project Summary
- {total_files} files, {total_loc} lines of code
- Languages: {langs}

## Key Files (by complexity)
{files}

## Public Interfaces (top 100)
{interfaces}

## Task
Produce comprehensive architecture documentation in Markdown covering:
1. **Overview** — Purpose, stakeholders, system context
2. **Architecture** — Component/container diagram, design patterns, key decisions
3. **Workflows** — Primary data/control flows, sequence diagrams (Mermaid)
4. **Boundaries** — External interfaces, API contracts, integration points
5. **Database** — Data model, storage patterns (if applicable)

Read specific source files as needed for deeper analysis.
Output each section with a ## heading. Use Mermaid diagrams where helpful.
"#,
        name = codebase.project_name,
        total_files = stats.total_files,
        total_loc = stats.total_loc,
        langs = lang_summary.join(", "),
        files = top_files.join("\n"),
        interfaces = interfaces.join("\n"),
    )
}
```

**Step 6: Run tests**

Run: `cd /c/codedev/litho-workspace && cargo test -p litho-codex 2>&1`
Expected: PASS

**Step 7: Commit**

```bash
git add -A
git commit -m "feat(litho-codex): codex-cli exec provider with prompt templates

Single codex invocation replaces 14 LLM round-trips.
Prompt includes pre-extracted AST data for focused analysis."
```

---

### Task 5: Wire CLI and End-to-End Test Against PC_AI

**Files:**
- Modify: `crates/litho-cli/src/main.rs` (finalize)
- Test: Run `litho extract` and `litho generate` against PC_AI

**Step 1: Build the CLI binary**

Run: `cd /c/codedev/litho-workspace && cargo build --release 2>&1`
Expected: Compiles successfully

**Step 2: Copy binary to ~/bin/**

Run: `cp target/release/litho.exe /c/Users/david/bin/litho.exe`

**Step 3: Test extraction against PC_AI (no LLM)**

Run: `litho extract /c/Users/david/PC_AI --format json > /tmp/pcai_extract.json 2>&1`
Expected: JSON output with files, interfaces, dependency graph. Should complete in <10s.

**Step 4: Validate extraction output**

Run: `cat /tmp/pcai_extract.json | python -c "import json,sys; d=json.load(sys.stdin); print(f'Files: {d[\"statistics\"][\"total_files\"]}, LOC: {d[\"statistics\"][\"total_loc\"]}')"`
Expected: Reasonable numbers (100+ files, 10000+ LOC)

**Step 5: Test documentation generation with codex**

Run: `litho generate /c/Users/david/PC_AI --provider codex --output /tmp/litho-pcai-docs/ 2>&1`
Expected: Markdown files in `/tmp/litho-pcai-docs/`

**Step 6: Commit**

```bash
git add -A
git commit -m "feat(litho-cli): wire extract + generate commands

End-to-end validated against PC_AI codebase."
```

---

### Task 6: Copy deepwiki-rs Legacy Code Into Workspace

**Files:**
- Copy: `/c/codedev/deepwiki-rs/src/` → `crates/litho-generator/src/`
- Copy: `/c/codedev/litho-book/src/` → `crates/litho-book/src/`
- Modify: Adjust module paths and imports

**Step 1: Create litho-generator crate from deepwiki-rs source**

Copy the existing pipeline code (preprocess, research, compose, outlet) into `litho-generator`, adjusting imports to use `litho-core` types where applicable.

**Step 2: Create litho-book crate from litho-book source**

Copy the web UI code.

**Step 3: Fix modelscope.cn default**

In `crates/litho-core/src/config.rs`, the default is already empty string. For the legacy `litho-generator` path, ensure it also reads from `litho-core` config.

**Step 4: Build full workspace**

Run: `cargo build --workspace 2>&1`
Expected: All crates compile

**Step 5: Commit**

```bash
git add -A
git commit -m "feat: import deepwiki-rs + litho-book as litho-generator + litho-book crates

Legacy rig-based pipeline preserved as fallback.
modelscope.cn default removed — requires explicit api_base_url."
```

---

### Task 7: PC_AI Doc Pipeline Integration

**Files:**
- Modify: `/c/Users/david/PC_AI/Tools/Invoke-DocPipeline.ps1`

**Step 1: Add Litho extraction step after existing PowerShellDocs step**

Add two new pipeline steps following the existing `Add-PipelineStep` pattern:

```powershell
# --- Litho AST Extraction (after PowerShellDocs) ---
$lithoExe = Get-Command litho -ErrorAction SilentlyContinue
if (-not $lithoExe) { $lithoExe = Join-Path $env:USERPROFILE 'bin\litho.exe' }
if (Test-Path $lithoExe) {
    try {
        & $lithoExe extract $repoRoot --format json | Out-File (Join-Path $reportsDir 'LITHO_EXTRACT.json') -Encoding utf8
        & $lithoExe extract $repoRoot --format summary | Out-File (Join-Path $reportsDir 'LITHO_EXTRACT_SUMMARY.md') -Encoding utf8
        Add-PipelineStep -Name 'LithoExtract' -Status 'Success' -Output (Join-Path $reportsDir 'LITHO_EXTRACT.json')
    } catch {
        Add-PipelineStep -Name 'LithoExtract' -Status 'Error' -Error $_.Exception.Message
    }
} else {
    Add-PipelineStep -Name 'LithoExtract' -Status 'Skipped' -Error 'litho binary not found'
}
```

**Step 2: Add optional Litho doc generation step**

```powershell
# --- Litho Documentation (optional, requires codex-cli) ---
if ($Mode -eq 'Full' -and (Test-Path $lithoExe)) {
    $lithoDocsDir = Join-Path $repoRoot 'docs\auto\litho'
    try {
        & $lithoExe generate $repoRoot --provider codex --output $lithoDocsDir
        Add-PipelineStep -Name 'LithoDocs' -Status 'Success' -Output $lithoDocsDir
    } catch {
        Add-PipelineStep -Name 'LithoDocs' -Status 'Warning' -Error $_.Exception.Message
    }
}
```

**Step 3: Test pipeline with new steps**

Run: `powershell.exe -NoProfile -Command ".\Tools\Invoke-DocPipeline.ps1 -Mode DocsOnly"`
Expected: LithoExtract step runs (or Skipped if binary not yet built)

**Step 4: Commit in PC_AI repo**

```bash
cd /c/Users/david/PC_AI
git add Tools/Invoke-DocPipeline.ps1
git commit -m "feat(docs): integrate litho extract + generate into doc pipeline

New pipeline steps: LithoExtract (AST analysis), LithoDocs (codex generation).
Both gracefully skip if litho binary not available."
```

---

### Task 8: CI/CD Workflows

**Files:**
- Create: `/c/codedev/litho-workspace/.github/workflows/ci.yml`

**Step 1: Create CI workflow**

```yaml
name: CI
on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  build:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo build --workspace
      - run: cargo test --workspace
      - run: cargo clippy --workspace -- -D warnings

  release:
    if: startsWith(github.ref, 'refs/tags/v')
    needs: build
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo build --release -p litho-cli
      - uses: softprops/action-gh-release@v2
        with:
          files: target/release/litho.exe
```

**Step 2: Commit**

```bash
git add -A
git commit -m "ci: add build + test + release workflow"
```
