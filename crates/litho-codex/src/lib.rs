/// Codex-CLI integration layer for litho-workspace.
///
/// This crate provides:
///
/// - [`provider::DocGenerator`] — the trait implemented by all doc-generation
///   backends.
/// - [`provider::DocumentSection`] — the output type produced by generators.
/// - [`exec::CodexExecGenerator`] — drives `codex exec` as a subprocess.
/// - [`prompts::build_prompt`] — assembles a structured prompt from an
///   [`litho_core::types::ExtractedCodebase`] snapshot.
pub mod exec;
pub mod prompts;
pub mod provider;

pub use provider::{DocGenerator, DocumentSection};
