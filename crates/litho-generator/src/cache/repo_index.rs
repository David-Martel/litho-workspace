use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoFileSnapshot {
    pub path: String,
    pub content_hash: String,
    pub file_size: u64,
    pub modified_unix: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoDiffPlan {
    pub unchanged: usize,
    pub changed_paths: Vec<String>,
    pub new_paths: Vec<String>,
    pub removed_paths: Vec<String>,
    pub git_changed_paths: Vec<String>,
    pub git_removed_paths: Vec<String>,
    pub previous_commit: Option<String>,
    pub current_commit: Option<String>,
}

impl RepoDiffPlan {
    pub fn total_affected(&self) -> usize {
        self.changed_paths.len() + self.new_paths.len() + self.removed_paths.len()
    }
}

pub struct RepoIndexStore {
    db_path: PathBuf,
    conn: Mutex<Connection>,
}

impl RepoIndexStore {
    pub fn open(db_path: PathBuf) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create repo index parent dir: {}",
                    parent.display()
                )
            })?;
        }
        let conn = Connection::open(&db_path)
            .with_context(|| format!("open sqlite: {}", db_path.display()))?;
        initialize_schema(&conn)?;
        Ok(Self {
            db_path,
            conn: Mutex::new(conn),
        })
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn last_commit(&self) -> Result<Option<String>> {
        let conn = self.conn.lock().expect("repo index mutex poisoned");
        let mut stmt = conn.prepare("SELECT value FROM repo_state WHERE key = 'last_commit'")?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            let value: String = row.get(0)?;
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    pub fn set_last_commit(&self, commit: &str) -> Result<()> {
        let conn = self.conn.lock().expect("repo index mutex poisoned");
        let now = unix_now();
        conn.execute(
            "INSERT INTO repo_state(key, value, updated_at) VALUES('last_commit', ?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
            params![commit, now],
        )?;
        Ok(())
    }

    pub fn diff_with_snapshots(&self, snapshots: &[RepoFileSnapshot]) -> Result<RepoDiffPlan> {
        let conn = self.conn.lock().expect("repo index mutex poisoned");
        let mut known = BTreeMap::<String, String>::new();
        let mut stmt = conn.prepare("SELECT path, content_hash FROM file_index")?;
        let rows = stmt.query_map([], |row| {
            let path: String = row.get(0)?;
            let hash: String = row.get(1)?;
            Ok((path, hash))
        })?;
        for row in rows {
            let (path, hash) = row?;
            known.insert(path, hash);
        }

        let mut seen = BTreeSet::<String>::new();
        let mut plan = RepoDiffPlan::default();
        for snap in snapshots {
            seen.insert(snap.path.clone());
            match known.get(&snap.path) {
                Some(old_hash) if old_hash == &snap.content_hash => {
                    plan.unchanged += 1;
                }
                Some(_) => plan.changed_paths.push(snap.path.clone()),
                None => plan.new_paths.push(snap.path.clone()),
            }
        }
        for known_path in known.keys() {
            if !seen.contains(known_path) {
                plan.removed_paths.push(known_path.clone());
            }
        }
        Ok(plan)
    }

    pub fn apply_snapshots(
        &self,
        snapshots: &[RepoFileSnapshot],
        commit: Option<&str>,
    ) -> Result<()> {
        let mut conn = self.conn.lock().expect("repo index mutex poisoned");
        let tx = conn.transaction()?;

        let mut current_paths = BTreeSet::<String>::new();
        for snap in snapshots {
            current_paths.insert(snap.path.clone());
            tx.execute(
                "INSERT INTO file_index(path, content_hash, file_size, modified_unix, updated_at)
                 VALUES(?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(path) DO UPDATE SET
                   content_hash=excluded.content_hash,
                   file_size=excluded.file_size,
                   modified_unix=excluded.modified_unix,
                   updated_at=excluded.updated_at",
                params![
                    snap.path,
                    snap.content_hash,
                    snap.file_size as i64,
                    snap.modified_unix,
                    unix_now(),
                ],
            )?;
        }

        let mut to_remove = Vec::<String>::new();
        let mut stmt = tx.prepare("SELECT path FROM file_index")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        for row in rows {
            let path = row?;
            if !current_paths.contains(&path) {
                to_remove.push(path);
            }
        }
        drop(stmt);
        for path in to_remove {
            tx.execute("DELETE FROM file_index WHERE path = ?1", params![path])?;
        }

        if let Some(value) = commit {
            tx.execute(
                "INSERT INTO repo_state(key, value, updated_at) VALUES('last_commit', ?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
                params![value, unix_now()],
            )?;
        }

        tx.commit()?;
        Ok(())
    }
}

fn initialize_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;
        CREATE TABLE IF NOT EXISTS repo_state (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS file_index (
            path TEXT PRIMARY KEY,
            content_hash TEXT NOT NULL,
            file_size INTEGER NOT NULL,
            modified_unix INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_file_index_hash ON file_index(content_hash);
        ",
    )
    .context("failed to initialize repo index schema")?;
    Ok(())
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn detect_git_diff(project_root: &Path, previous_commit: Option<&str>) -> RepoDiffPlan {
    let mut plan = RepoDiffPlan::default();

    let current_commit = git_current_head(project_root);
    plan.current_commit = current_commit.clone();
    plan.previous_commit = previous_commit.map(std::string::ToString::to_string);

    let Some(prev) = previous_commit else {
        return plan;
    };
    let Some(curr) = &current_commit else {
        return plan;
    };
    if prev == curr {
        return plan;
    }

    plan.git_changed_paths = git_diff_names(project_root, prev, curr, "ACMR");
    plan.git_removed_paths = git_diff_names(project_root, prev, curr, "D");
    plan
}

fn git_current_head(project_root: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project_root)
        .arg("rev-parse")
        .arg("HEAD")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let head = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if head.is_empty() { None } else { Some(head) }
}

fn git_diff_names(project_root: &Path, previous: &str, current: &str, filter: &str) -> Vec<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project_root)
        .arg("diff")
        .arg("--name-only")
        .arg(format!("--diff-filter={filter}"))
        .arg(previous)
        .arg(current)
        .output();

    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(std::string::ToString::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn repo_index_schema_and_diff_work() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("repo-index.sqlite3");
        let store = RepoIndexStore::open(db).unwrap();

        let initial = vec![RepoFileSnapshot {
            path: "src/main.rs".to_string(),
            content_hash: "aaa".to_string(),
            file_size: 10,
            modified_unix: 1,
        }];
        let first = store.diff_with_snapshots(&initial).unwrap();
        assert_eq!(first.new_paths, vec!["src/main.rs".to_string()]);
        assert_eq!(first.total_affected(), 1);
        store.apply_snapshots(&initial, Some("deadbeef")).unwrap();
        assert_eq!(store.last_commit().unwrap().as_deref(), Some("deadbeef"));

        let updated = vec![RepoFileSnapshot {
            path: "src/main.rs".to_string(),
            content_hash: "bbb".to_string(),
            file_size: 11,
            modified_unix: 2,
        }];
        let second = store.diff_with_snapshots(&updated).unwrap();
        assert_eq!(second.changed_paths, vec!["src/main.rs".to_string()]);
        assert_eq!(second.total_affected(), 1);
    }
}
