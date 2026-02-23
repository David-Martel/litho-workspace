use crate::prompts;
use crate::provider::{DocGenerator, DocumentSection};
use async_trait::async_trait;
use litho_core::types::ExtractedCodebase;
use std::path::Path;
use std::process::Command;

/// A [`DocGenerator`] that delegates to the `codex exec` CLI tool.
///
/// `codex exec` is invoked as a subprocess with `--full-auto`, which lets it
/// read source files in the project directory and write the result to a
/// Markdown file in the output directory.
///
/// If `codex` is not installed the subprocess will fail to start and the
/// method returns an [`anyhow::Error`] — it never panics.
///
/// # Example
///
/// ```no_run
/// use litho_codex::exec::CodexExecGenerator;
/// use litho_codex::provider::DocGenerator;
///
/// let gen = CodexExecGenerator {
///     model: "o3".into(),
///     sandbox: "read-only".into(),
/// };
/// ```
pub struct CodexExecGenerator {
    /// Codex model identifier (e.g. `"o3"`, `"o4-mini"`).
    /// When empty the `codex` binary uses its built-in default.
    pub model: String,
    /// Sandbox policy passed to `codex exec -s`.
    pub sandbox: String,
}

impl Default for CodexExecGenerator {
    fn default() -> Self {
        Self {
            model: String::new(),
            sandbox: "read-only".into(),
        }
    }
}

#[async_trait]
impl DocGenerator for CodexExecGenerator {
    /// Run `codex exec` for the given `extracted` codebase snapshot.
    ///
    /// # Errors
    ///
    /// - Returns an error if the `codex` binary cannot be found or fails to
    ///   start (e.g. not installed).
    /// - Returns an error if `codex exec` exits with a non-zero status.
    /// - Returns an error if the output file cannot be read after a
    ///   successful run.
    async fn generate(
        &self,
        extracted: &ExtractedCodebase,
        project_path: &Path,
        output_path: &Path,
    ) -> anyhow::Result<Vec<DocumentSection>> {
        let prompt = prompts::build_prompt(extracted, "full");
        let output_file = output_path.join("codex-output.md");

        let mut cmd = Command::new("codex");
        cmd.arg("exec")
            .arg("--full-auto")
            .arg("-C")
            .arg(project_path)
            .arg("-s")
            .arg(&self.sandbox)
            .arg("-o")
            .arg(&output_file);

        if !self.model.is_empty() {
            cmd.arg("-m").arg(&self.model);
        }

        cmd.arg(&prompt);

        let output = cmd.output().map_err(|e| {
            anyhow::anyhow!(
                "failed to launch `codex` — is it installed and on PATH? ({})",
                e
            )
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("codex exec failed (exit {}): {stderr}", output.status);
        }

        // Prefer the dedicated output file; fall back to stdout.
        let content = std::fs::read_to_string(&output_file).unwrap_or_else(|_| {
            String::from_utf8_lossy(&output.stdout).into_owned()
        });

        Ok(vec![DocumentSection {
            title: "Architecture Documentation".into(),
            content,
            filename: "architecture.md".into(),
        }])
    }
}

/// Convenience wrapper used by the CLI.
///
/// Invokes [`CodexExecGenerator`] with default settings, treating the current
/// directory as the project root.
///
/// # Errors
///
/// Propagates any error from [`CodexExecGenerator::generate`].
pub async fn generate(
    extracted: &ExtractedCodebase,
    output_path: &Path,
) -> anyhow::Result<Vec<DocumentSection>> {
    let generator = CodexExecGenerator::default();
    let project_path = Path::new(".");
    generator.generate(extracted, project_path, output_path).await
}
