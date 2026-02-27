# GEMINI.md - Litho Workspace Context

This workspace, `litho-workspace`, is a high-performance, AI-driven ecosystem for documentation generation and search. It is primarily built with **Rust**, with some TypeScript/Bun components for specialized vector search.

## Project Overview

The project aims to automate the creation and browsing of professional architecture documentation (C4 model) from source code. It consists of several key modules:

- **Litho (deepwiki-rs):** The core AI engine that analyzes codebases and generates documentation.
- **Litho Book:** A modern Markdown reader (Rust + Axum) for browsing generated docs with Mermaid chart support.
- **QMD (Quick Markdown Search):** A TypeScript/Bun-based full-text and vector search engine using SQLite (sqlite-vec) and Ollama/node-llama-cpp.
- **Crates:** A modular Rust workspace containing core logic, extraction, codex, and generators.

### Tech Stack
- **Backend:** Rust (Axum, Tokio, Clap, Serde, Tree-sitter).
- **Search:** TypeScript/Bun, SQLite (with vector extensions), Ollama.
- **AI/LLM:** Integrated support for Ollama and various LLM providers for code analysis and embeddings.
- **Database:** PostgreSQL (configured in `qmd.config.json`).

## Building and Running

### Rust Workspace (Core & CLI)
The main entry point for the CLI is in `crates/litho-cli`.

- **Build Workspace:** `cargo build --workspace`
- **Run Tests:** `cargo test --workspace`
- **Lint:** `cargo clippy --workspace`
- **Build Release CLI:** `cargo build --release -p litho-cli` (Output: `target/release/litho.exe`)

### Litho Book
- **Run:** `cargo run -p litho-book -- --docs-dir <DOCS_DIR> --open`

### QMD (TypeScript/Bun)
Located in `third_party/qmd-ts`.
- **Install:** `bun install`
- **Run Indexer:** `bun run index`
- **Search:** `bun run search <QUERY>`
- **Vector Search:** `bun run vsearch <QUERY>`

## Development Conventions

- **Code Style:** Standard Rust idioms are followed. `clippy` is enforced in CI.
- **CI/CD:** GitHub Actions (`.github/workflows/ci.yml`) handles building, testing, clippy, and release artifact generation for Windows.
- **Architecture:** Follows a 4-stage processing pipeline: Preprocessing -> Intelligent Research -> Documentation Generation -> Verification & Enhancement.
- **Dependencies:** Managed via workspace-level `Cargo.toml`. Tree-sitter is used for multi-language parsing (Rust, TS, Python, C#, etc.).
- **Configurations:** 
  - `qmd.config.json`: Database and LLM provider settings.
  - `.env`: Used for environment-specific secrets (see `.env.example`).

## Directory Structure Highlights
- `crates/`: Modular Rust libraries and tools.
- `deepwiki-rs/`: Main engine documentation and assets.
- `litho-book/`: Documentation viewer project.
- `third_party/qmd-ts/`: Search engine component.
- `scripts/`: PowerShell scripts for quality gates, benchmarks, and database bootstrapping.
