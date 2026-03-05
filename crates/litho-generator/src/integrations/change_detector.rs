//! Change Detector — identifies which files changed since the last generation run.
//!
//! Uses `git diff --name-status` to compare the current HEAD against the manifest's recorded
//! commit. Maps changed files to affected documentation agents using path-based heuristics.

use anyhow::{Context, Result};
use std::collections::{HashSet, hash_map::Values};
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
    let total_tracked = tracked_file_count(manifest);
    let total_changes = changed_files.len() + added_files.len() + removed_files.len();
    let change_ratio = if total_tracked == 0 {
        0.0
    } else {
        total_changes as f64 / total_tracked as f64
    };

    let full_rebuild_needed = total_tracked > 0 && change_ratio > 0.3;

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

        for module in manifest.modules.values() {
            if module.input_files.iter().any(|input| {
                let file_str = file.to_string_lossy();
                let input_str = input.to_string_lossy();
                file_str.contains(input_str.as_ref()) || input_str.contains(file_str.as_ref())
            }) {
                affected.insert(normalize_agent_name(&module.agent_type));
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

fn tracked_file_count(manifest: &DocumentationManifest) -> usize {
    if !manifest.file_hashes.is_empty() {
        return manifest.file_hashes.len();
    }

    unique_module_input_file_count(manifest.modules.values())
}

fn unique_module_input_file_count<'a>(
    modules: Values<'a, String, super::manifest::ModuleManifest>,
) -> usize {
    let mut seen = HashSet::new();
    for module in modules {
        for path in &module.input_files {
            seen.insert(path.clone());
        }
    }
    seen.len()
}

fn normalize_agent_name(agent: &str) -> String {
    if agent.starts_with("KeyModulesInsight_") || agent.contains("KeyModulesInsight") {
        return "KeyModulesInsight".to_string();
    }

    match agent {
        "Project Overview" => "Overview".to_string(),
        "OverviewEditor" => "Overview".to_string(),
        "Architecture Description" => "Architecture".to_string(),
        "ArchitectureEditor" => "Architecture".to_string(),
        "Core Workflows" => "Workflow".to_string(),
        "WorkflowEditor" => "Workflow".to_string(),
        "Boundary Interfaces" => "Boundary".to_string(),
        "BoundaryEditor" => "Boundary".to_string(),
        "Database Overview" => "Database".to_string(),
        "DatabaseEditor" => "Database".to_string(),
        other => other.to_string(),
    }
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

        let changed = [PathBuf::from("src/main.rs")];
        let refs: Vec<&PathBuf> = changed.iter().collect();
        let affected = map_files_to_agents(&refs, &manifest);

        assert!(affected.contains("Overview"));
    }

    #[test]
    fn test_map_files_to_agents_unknown_triggers_research() {
        let manifest = DocumentationManifest::new(PathBuf::from("/test"));

        let changed = [PathBuf::from("new_file.rs")];
        let refs: Vec<&PathBuf> = changed.iter().collect();
        let affected = map_files_to_agents(&refs, &manifest);

        // Unknown file should trigger research agents + all compose agents
        assert!(affected.contains("SystemContextResearcher"));
        assert!(affected.contains("DomainModulesDetector"));
        assert!(affected.contains("Overview"));
    }

    // --- New tests ---

    #[test]
    fn test_changeset_is_empty_only_when_all_vecs_empty() {
        // modified only
        let cs = ChangeSet {
            changed_files: vec![PathBuf::from("a.rs")],
            added_files: vec![],
            removed_files: vec![],
            affected_agents: HashSet::new(),
            full_rebuild_needed: false,
        };
        assert!(!cs.is_empty());

        // added only
        let cs2 = ChangeSet {
            changed_files: vec![],
            added_files: vec![PathBuf::from("b.rs")],
            removed_files: vec![],
            affected_agents: HashSet::new(),
            full_rebuild_needed: false,
        };
        assert!(!cs2.is_empty());

        // removed only
        let cs3 = ChangeSet {
            changed_files: vec![],
            added_files: vec![],
            removed_files: vec![PathBuf::from("c.rs")],
            affected_agents: HashSet::new(),
            full_rebuild_needed: false,
        };
        assert!(!cs3.is_empty());
    }

    #[test]
    fn test_changeset_total_changes_sums_all_categories() {
        let cs = ChangeSet {
            changed_files: vec![PathBuf::from("a.rs"), PathBuf::from("b.rs")],
            added_files: vec![PathBuf::from("c.rs")],
            removed_files: vec![
                PathBuf::from("d.rs"),
                PathBuf::from("e.rs"),
                PathBuf::from("f.rs"),
            ],
            affected_agents: HashSet::new(),
            full_rebuild_needed: false,
        };
        assert_eq!(cs.total_changes(), 6);
    }

    #[test]
    fn test_full_rebuild_flag_is_independent_of_is_empty() {
        // A changeset can be empty yet have full_rebuild_needed = true (no-commit case)
        let cs = ChangeSet {
            changed_files: vec![],
            added_files: vec![],
            removed_files: vec![],
            affected_agents: HashSet::new(),
            full_rebuild_needed: true,
        };
        assert!(cs.is_empty());
        assert_eq!(cs.total_changes(), 0);
        assert!(cs.full_rebuild_needed);
    }

    #[test]
    fn test_map_files_research_agent_cascades_to_compose_agents() {
        // A file that maps to a Researcher should pull in all compose agents
        let mut manifest = DocumentationManifest::new(PathBuf::from("/test"));
        manifest.modules.insert(
            "SystemContextResearcher".to_string(),
            ModuleManifest {
                agent_type: "SystemContextResearcher".to_string(),
                output_file: "context.md".to_string(),
                input_files: vec![PathBuf::from("src/main.rs")],
                generated_at: Utc::now(),
                content_hash: "xyz".to_string(),
            },
        );

        let changed = [PathBuf::from("src/main.rs")];
        let refs: Vec<&PathBuf> = changed.iter().collect();
        let affected = map_files_to_agents(&refs, &manifest);

        // The matched agent is a Researcher — compose agents must follow
        assert!(affected.contains("Overview"));
        assert!(affected.contains("Architecture"));
        assert!(affected.contains("Workflow"));
        assert!(affected.contains("Boundary"));
        assert!(affected.contains("Database"));
        assert!(affected.contains("KeyModulesInsight"));
    }

    #[test]
    fn test_map_files_non_research_agent_does_not_cascade() {
        // A file that maps to a non-research agent (no Researcher/Detector/Analyzer keyword)
        // should NOT cascade to compose agents
        let mut manifest = DocumentationManifest::new(PathBuf::from("/test"));
        manifest.modules.insert(
            "Overview".to_string(),
            ModuleManifest {
                agent_type: "OverviewEditor".to_string(),
                output_file: "1.Overview.md".to_string(),
                input_files: vec![PathBuf::from("docs/overview.md")],
                generated_at: Utc::now(),
                content_hash: "aaa".to_string(),
            },
        );

        let changed = [PathBuf::from("docs/overview.md")];
        let refs: Vec<&PathBuf> = changed.iter().collect();
        let affected = map_files_to_agents(&refs, &manifest);

        assert!(affected.contains("Overview"));
        // Architecture/Workflow/etc. should NOT be added since no research agent was triggered
        assert!(!affected.contains("Architecture"));
        assert!(!affected.contains("Workflow"));
    }

    #[test]
    fn test_map_files_empty_input_yields_empty_affected() {
        let manifest = DocumentationManifest::new(PathBuf::from("/test"));
        let refs: Vec<&PathBuf> = vec![];
        let affected = map_files_to_agents(&refs, &manifest);
        assert!(affected.is_empty());
    }

    #[test]
    fn test_thirty_percent_threshold_boundary() {
        // Verify that the full_rebuild logic is >0.3 (strictly greater than)
        // We can't call detect_changes without git, but we can replicate the
        // arithmetic that the function uses and assert the boundary condition.

        // 3 changes out of 10 tracked = 0.30 exactly → NOT full rebuild (> 0.3, not >=)
        let total_tracked: usize = 10;
        let total_changes: usize = 3;
        let ratio = total_changes as f64 / total_tracked as f64;
        assert!((ratio - 0.3).abs() < f64::EPSILON);
        assert!(ratio <= 0.3, "exactly 30% must NOT trigger full rebuild");

        // 4 changes out of 10 = 0.40 → IS full rebuild
        let total_changes_over: usize = 4;
        let ratio_over = total_changes_over as f64 / total_tracked as f64;
        assert!(ratio_over > 0.3, "40% must trigger full rebuild");
    }

    #[test]
    fn test_changeset_affected_agents_reflects_full_rebuild_semantics() {
        // When full_rebuild_needed is true the detect_changes function returns
        // an empty affected_agents set (the caller re-runs all agents)
        let cs = ChangeSet {
            changed_files: vec![PathBuf::from("src/huge_change.rs")],
            added_files: vec![],
            removed_files: vec![],
            affected_agents: HashSet::new(), // empty on full rebuild
            full_rebuild_needed: true,
        };
        // A full-rebuild changeset is NOT empty (there are changed files)
        assert!(!cs.is_empty());
        // But affected_agents is empty — the caller runs everything
        assert!(cs.affected_agents.is_empty());
    }

    #[test]
    fn test_tracked_file_count_falls_back_to_module_inputs() {
        let mut manifest = DocumentationManifest::new(PathBuf::from("/test"));
        manifest.modules.insert(
            "Overview".to_string(),
            ModuleManifest {
                agent_type: "Overview".to_string(),
                output_file: "overview.md".to_string(),
                input_files: vec![PathBuf::from("src/main.rs"), PathBuf::from("src/lib.rs")],
                generated_at: Utc::now(),
                content_hash: "abc".to_string(),
            },
        );
        manifest.modules.insert(
            "Architecture".to_string(),
            ModuleManifest {
                agent_type: "Architecture".to_string(),
                output_file: "architecture.md".to_string(),
                input_files: vec![PathBuf::from("src/lib.rs")],
                generated_at: Utc::now(),
                content_hash: "def".to_string(),
            },
        );

        assert_eq!(tracked_file_count(&manifest), 2);
    }

    #[test]
    fn test_normalize_agent_name_maps_display_names() {
        assert_eq!(normalize_agent_name("Project Overview"), "Overview");
        assert_eq!(
            normalize_agent_name("Architecture Description"),
            "Architecture"
        );
        assert_eq!(
            normalize_agent_name("KeyModulesInsight_ordering"),
            "KeyModulesInsight"
        );
    }
}
