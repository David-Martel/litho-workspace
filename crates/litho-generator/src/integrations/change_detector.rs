//! Change Detector — identifies which files changed since the last generation run.
//!
//! Uses `git diff --name-status` to compare the current HEAD against the manifest's recorded
//! commit. Maps changed files to affected documentation agents using path-based heuristics.

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::manifest::DocumentationManifest;

/// Result of change detection: which files changed and which agents are affected.
#[derive(Debug, Clone)]
pub struct ChangeSet {
    /// Files that were modified since the manifest commit
    pub changed_files: Vec<PathBuf>,
    /// Files that were added since the manifest commit
    pub added_files: Vec<PathBuf>,
    /// Files that were removed since the manifest commit
    pub removed_files: Vec<PathBuf>,
    /// Agent types that need re-running based on affected files
    pub affected_agents: HashSet<String>,
    /// Whether ALL agents should be re-run (e.g., >30% files changed)
    pub full_rebuild_needed: bool,
}

impl ChangeSet {
    /// Returns true if no files changed.
    pub fn is_empty(&self) -> bool {
        self.changed_files.is_empty()
            && self.added_files.is_empty()
            && self.removed_files.is_empty()
    }

    /// Total number of changed files (modified + added + removed).
    pub fn total_changes(&self) -> usize {
        self.changed_files.len() + self.added_files.len() + self.removed_files.len()
    }
}

/// Detect changes between the manifest's recorded state and current HEAD.
pub async fn detect_changes(
    project_path: &Path,
    manifest: &DocumentationManifest,
) -> Result<ChangeSet> {
    let git_commit = match &manifest.git_commit {
        Some(commit) => commit.clone(),
        None => {
            // No previous commit recorded — full rebuild needed
            return Ok(ChangeSet {
                changed_files: Vec::new(),
                added_files: Vec::new(),
                removed_files: Vec::new(),
                affected_agents: HashSet::new(),
                full_rebuild_needed: true,
            });
        }
    };

    // Run git diff to get changed files
    let output = tokio::process::Command::new("git")
        .args(["diff", "--name-status", &format!("{}..HEAD", git_commit)])
        .current_dir(project_path)
        .output()
        .await
        .context("Failed to run git diff")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // If the commit doesn't exist (e.g., after a rebase), trigger full rebuild
        if stderr.contains("unknown revision") || stderr.contains("bad revision") {
            println!(
                "⚠️  Manifest commit {} no longer exists, triggering full rebuild",
                git_commit
            );
            return Ok(ChangeSet {
                changed_files: Vec::new(),
                added_files: Vec::new(),
                removed_files: Vec::new(),
                affected_agents: HashSet::new(),
                full_rebuild_needed: true,
            });
        }
        anyhow::bail!("git diff failed: {}", stderr);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut changed_files = Vec::new();
    let mut added_files = Vec::new();
    let mut removed_files = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.splitn(2, '\t').collect();
        if parts.len() != 2 {
            continue;
        }
        let status = parts[0].trim();
        let file_path = PathBuf::from(parts[1].trim());

        match status {
            "M" => changed_files.push(file_path),
            "A" => added_files.push(file_path),
            "D" => removed_files.push(file_path),
            s if s.starts_with('R') => {
                // Rename: treat as delete + add
                let rename_parts: Vec<&str> = parts[1].splitn(2, '\t').collect();
                if rename_parts.len() == 2 {
                    removed_files.push(PathBuf::from(rename_parts[0].trim()));
                    added_files.push(PathBuf::from(rename_parts[1].trim()));
                } else {
                    changed_files.push(file_path);
                }
            }
            _ => changed_files.push(file_path),
        }
    }

    // Determine affected agents
    let total_tracked = manifest.file_hashes.len().max(1);
    let total_changes = changed_files.len() + added_files.len() + removed_files.len();
    let change_ratio = total_changes as f64 / total_tracked as f64;

    let full_rebuild_needed = change_ratio > 0.3;

    let affected_agents = if full_rebuild_needed {
        // More than 30% changed — re-run everything
        HashSet::new()
    } else {
        // Map changed files to affected agents
        let all_changed: Vec<&PathBuf> = changed_files
            .iter()
            .chain(added_files.iter())
            .chain(removed_files.iter())
            .collect();
        map_files_to_agents(&all_changed, manifest)
    };

    Ok(ChangeSet {
        changed_files,
        added_files,
        removed_files,
        affected_agents,
        full_rebuild_needed,
    })
}

/// Get the current git commit hash (short form).
pub async fn get_git_commit(project_path: &Path) -> Option<String> {
    let output = tokio::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(project_path)
        .output()
        .await
        .ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

/// Get the current git branch name.
pub async fn get_git_branch(project_path: &Path) -> Option<String> {
    let output = tokio::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(project_path)
        .output()
        .await
        .ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

/// Map changed files to affected documentation agents.
///
/// Uses a conservative heuristic: if a file belongs to any module's input_files, that module
/// is affected. Files that don't map to any module trigger re-running the research agents
/// (SystemContextResearcher, DomainModulesDetector) since they affect global understanding.
fn map_files_to_agents(
    changed_files: &[&PathBuf],
    manifest: &DocumentationManifest,
) -> HashSet<String> {
    let mut affected = HashSet::new();

    for file in changed_files {
        let mut mapped = false;

        for (agent_key, module) in &manifest.modules {
            if module.input_files.iter().any(|input| {
                let file_str = file.to_string_lossy();
                let input_str = input.to_string_lossy();
                file_str.contains(input_str.as_ref()) || input_str.contains(file_str.as_ref())
            }) {
                affected.insert(agent_key.clone());
                mapped = true;
            }
        }

        if !mapped {
            // Unknown file changed — conservatively trigger global research agents
            affected.insert("SystemContextResearcher".to_string());
            affected.insert("DomainModulesDetector".to_string());
        }
    }

    // If research agents are affected, their downstream compose agents must also re-run
    let research_agents: Vec<String> = affected
        .iter()
        .filter(|a| {
            a.contains("Researcher")
                || a.contains("Detector")
                || a.contains("Analyzer")
                || a.contains("Insight")
        })
        .cloned()
        .collect();

    if !research_agents.is_empty() {
        // Conservatively: if any research agent is affected, all compose agents re-run
        affected.insert("Overview".to_string());
        affected.insert("Architecture".to_string());
        affected.insert("Workflow".to_string());
        affected.insert("Boundary".to_string());
        affected.insert("Database".to_string());
        affected.insert("KeyModulesInsight".to_string());
    }

    affected
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::manifest::ModuleManifest;
    use chrono::Utc;

    #[test]
    fn test_changeset_empty() {
        let cs = ChangeSet {
            changed_files: vec![],
            added_files: vec![],
            removed_files: vec![],
            affected_agents: HashSet::new(),
            full_rebuild_needed: false,
        };
        assert!(cs.is_empty());
        assert_eq!(cs.total_changes(), 0);
    }

    #[test]
    fn test_map_files_to_agents_known_file() {
        let mut manifest = DocumentationManifest::new(PathBuf::from("/test"));
        manifest.modules.insert(
            "Overview".to_string(),
            ModuleManifest {
                agent_type: "OverviewEditor".to_string(),
                output_file: "1.Overview.md".to_string(),
                input_files: vec![PathBuf::from("src/main.rs")],
                generated_at: Utc::now(),
                content_hash: "abc".to_string(),
            },
        );

        let changed = vec![PathBuf::from("src/main.rs")];
        let refs: Vec<&PathBuf> = changed.iter().collect();
        let affected = map_files_to_agents(&refs, &manifest);

        assert!(affected.contains("Overview"));
    }

    #[test]
    fn test_map_files_to_agents_unknown_triggers_research() {
        let manifest = DocumentationManifest::new(PathBuf::from("/test"));

        let changed = vec![PathBuf::from("new_file.rs")];
        let refs: Vec<&PathBuf> = changed.iter().collect();
        let affected = map_files_to_agents(&refs, &manifest);

        // Unknown file should trigger research agents + all compose agents
        assert!(affected.contains("SystemContextResearcher"));
        assert!(affected.contains("DomainModulesDetector"));
        assert!(affected.contains("Overview"));
    }
}
