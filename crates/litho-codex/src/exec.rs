use anyhow::Context;
use crate::prompts;
use crate::provider::{DocGenerator, DocumentSection};
use async_trait::async_trait;
use litho_core::env::LithoEnv;
use litho_core::types::ExtractedCodebase;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A [`DocGenerator`] that delegates to the `codex exec` CLI tool.
pub struct CodexExecGenerator {
    /// Codex model identifier (e.g. `"o3"`, `"o4-mini"`).
    pub model: String,
    /// Sandbox policy passed to `codex exec -s`.
    pub sandbox: String,
    /// Optional override for the `codex` binary path.
    pub binary_path: Option<PathBuf>,
}

impl Default for CodexExecGenerator {
    fn default() -> Self {
        Self {
            model: LithoEnv::codex_model().unwrap_or_default(),
            sandbox: "read-only".into(),
            binary_path: None,
        }
    }
}

impl CodexExecGenerator {
    /// Resolve the `codex` binary path.
    fn resolve_binary(&self) -> PathBuf {
        if let Some(ref path) = self.binary_path {
            return path.clone();
        }

        if let Some(env_path) = LithoEnv::codex_bin() {
            return env_path;
        }

        // Check current directory
        if Path::new("codex.exe").exists() {
            return PathBuf::from("codex.exe");
        }

        // Check executable directory
        if let Ok(current_exe) = env::current_exe() {
            if let Some(parent) = current_exe.parent() {
                let local_codex = parent.join("codex.exe");
                if local_codex.exists() {
                    return local_codex;
                }
            }
        }

        PathBuf::from("codex")
    }
}

#[async_trait]
impl DocGenerator for CodexExecGenerator {
    async fn validate_readiness(&self) -> anyhow::Result<()> {
        let bin = self.resolve_binary();
        let output = Command::new(&bin)
            .arg("--version")
            .output()
            .map_err(|e| {
                anyhow::anyhow!(
                    "codex binary not found or not executable at {:?}: {}",
                    bin, e
                )
            })?;

        if !output.status.success() {
            anyhow::bail!("codex --version failed with exit {}", output.status);
        }

        Ok(())
    }

    async fn generate(
        &self,
        extracted: &ExtractedCodebase,
        project_path: &Path,
        output_path: &Path,
    ) -> anyhow::Result<Vec<DocumentSection>> {
        let bin = self.resolve_binary();
        let prompt = prompts::build_prompt_with_optional_qmd(extracted, "full", project_path);
        let output_file = output_path.join("codex-output.md");

        let mut cmd = Command::new(bin);
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
                "failed to launch codex process: {}",
                e
            )
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("codex exec failed (exit {}): {stderr}", output.status);
        }

        // Prefer the dedicated output file; fall back to stdout.
        let content = std::fs::read_to_string(&output_file)
            .unwrap_or_else(|_| String::from_utf8_lossy(&output.stdout).into_owned());

        Ok(vec![DocumentSection {
            title: "Architecture Documentation".into(),
            content,
            filename: "architecture.md".into(),
        }])
    }
}

/// A [`DocGenerator`] that calls the `codex` library directly.
pub struct CodexLibGenerator {
    /// Codex model identifier.
    pub model: String,
}

impl Default for CodexLibGenerator {
    fn default() -> Self {
        Self {
            model: LithoEnv::codex_model().unwrap_or_default(),
        }
    }
}

#[async_trait]
impl DocGenerator for CodexLibGenerator {
    async fn validate_readiness(&self) -> anyhow::Result<()> {
        // Since we are linked against the library, we are "ready" if we can initialize config.
        // We'll do a lightweight check by attempting to find codex home.
        codex_core::config::find_codex_home()
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("Failed to initialize codex library environment: {}", e))
    }

    async fn generate(
        &self,
        extracted: &ExtractedCodebase,
        project_path: &Path,
        output_path: &Path,
    ) -> anyhow::Result<Vec<DocumentSection>> {
        let prompt = prompts::build_prompt_with_optional_qmd(extracted, "full", project_path);
        let output_file = output_path.join("codex-output.md");
        let model = self.model.clone();
        let project_path = project_path.to_path_buf();
        let output_file_for_task = output_file.clone();

        // codex_exec::run_main uses non-Send types internally (dyn EventProcessor,
        // dyn PullProgressReporter), so we cannot .await it inside an async_trait
        // method (which requires Send futures). Work around this by running it on
        // a dedicated single-threaded runtime inside spawn_blocking.
        tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to create tokio runtime: {}", e))?;

            let local = tokio::task::LocalSet::new();
            local.block_on(&rt, async {
                let cli = codex_exec::Cli {
                    command: None,
                    images: vec![],
                    model: Some(model),
                    oss: false,
                    oss_provider: None,
                    sandbox_mode: Some(codex_utils_cli::SandboxModeCliArg::WorkspaceWrite),
                    config_profile: None,
                    full_auto: true,
                    dangerously_bypass_approvals_and_sandbox: false,
                    cwd: Some(project_path),
                    skip_git_repo_check: true,
                    add_dir: vec![],
                    ephemeral: true,
                    output_schema: None,
                    config_overrides: Default::default(),
                    color: codex_exec::Color::Auto,
                    json: false,
                    last_message_file: Some(output_file_for_task),
                    prompt: Some(prompt),
                };

                codex_exec::run_main(cli, None).await
            })
        })
        .await
        .map_err(|e| anyhow::anyhow!("Codex library thread panicked: {}", e))??;

        let content = std::fs::read_to_string(&output_file)
            .context("Failed to read documentation output from library call")?;

        Ok(vec![DocumentSection {
            title: "Architecture Documentation".into(),
            content,
            filename: "architecture.md".into(),
        }])
    }
}

/// Convenience wrapper used by the CLI.
///
/// Invokes [`CodexLibGenerator`] with default settings, treating the current
/// directory as the project root.
pub async fn generate(
    extracted: &ExtractedCodebase,
    output_path: &Path,
) -> anyhow::Result<Vec<DocumentSection>> {
    let generator = CodexLibGenerator::default();
    let project_path = Path::new(".");
    generator
        .generate(extracted, project_path, output_path)
        .await
}
