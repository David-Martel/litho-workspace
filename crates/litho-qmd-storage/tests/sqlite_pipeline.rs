use litho_qmd_core::{QmdService, QmdStore, SearchOptions};
use litho_qmd_llm::NoopLlmEngine;
use litho_qmd_storage::{AutoQmdStore, QmdBackendKind, SqliteQmdStore};
use serial_test::serial;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn setup_sqlite_store() -> (TempDir, SqliteQmdStore, PathBuf, String) {
    let temp = tempfile::tempdir().expect("tempdir");
    let docs = temp.path().join("docs");
    fs::create_dir_all(&docs).expect("create docs");

    let cfg_path = temp.path().join("index.yml");
    let db_path = temp.path().join("index.sqlite3");
    let collection = "notes".to_string();

    let docs_norm = docs.to_string_lossy().replace('\\', "/");
    let yaml =
        format!("collections:\n  {collection}:\n    path: '{docs_norm}'\n    pattern: '**/*.md'\n");
    fs::write(&cfg_path, yaml).expect("write config");

    let store = SqliteQmdStore::open_with_paths(&db_path, &cfg_path).expect("open sqlite store");
    (temp, store, docs, collection)
}

struct ProcessGuard {
    old_dir: PathBuf,
    env_restore: Vec<(String, Option<String>)>,
}

impl ProcessGuard {
    fn new(temp_dir: &Path, keys: &[&str]) -> Self {
        let old_dir = std::env::current_dir().expect("current dir");
        std::env::set_current_dir(temp_dir).expect("set current dir");

        let mut env_restore = Vec::new();
        for key in keys {
            let key = key.to_string();
            let existing = std::env::var(&key).ok();
            env_restore.push((key.clone(), existing));
            unsafe { std::env::remove_var(&key) };
        }

        Self {
            old_dir,
            env_restore,
        }
    }
}

impl Drop for ProcessGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.old_dir);
        for (key, value) in &self.env_restore {
            match value {
                Some(v) => unsafe { std::env::set_var(key, v) },
                None => unsafe { std::env::remove_var(key) },
            }
        }
    }
}

#[test]
fn sqlite_ingest_embed_and_search_pipeline() {
    let (_temp, store, docs, collection) = setup_sqlite_store();

    fs::write(
        docs.join("alpha.md"),
        "# Rust Ownership\n\nThe borrow checker enforces aliasing rules.",
    )
    .expect("write alpha");
    fs::write(
        docs.join("beta.md"),
        "# Search Tuning\n\nFalse positives and false negatives can be balanced.",
    )
    .expect("write beta");

    let ingest = store.ingest_collections(false).expect("ingest");
    assert_eq!(ingest.indexed_documents, 2);

    let hits = store
        .search_bm25(
            "borrow checker",
            &SearchOptions {
                limit: 5,
                min_score: 0.0,
                collection: Some(collection.clone()),
            },
        )
        .expect("bm25");
    assert!(!hits.is_empty(), "bm25 should return results");

    let embed = store.embed_native(false).expect("embed");
    assert!(
        embed.embedded + embed.skipped >= 2,
        "expected native embeddings to exist for docs (embedded or skipped as already-present)"
    );

    let vhits = store
        .search_vector(
            "ownership aliasing rules",
            &SearchOptions {
                limit: 5,
                min_score: 0.0,
                collection: Some(collection.clone()),
            },
        )
        .expect("vsearch");
    assert!(!vhits.is_empty(), "vector search should return results");

    let service = QmdService::new(store, NoopLlmEngine);
    let qhits = service
        .query(
            "how does rust ownership work",
            SearchOptions {
                limit: 5,
                min_score: 0.0,
                collection: Some(collection),
            },
        )
        .expect("query");
    assert!(
        !qhits.results.is_empty(),
        "hybrid query should return results"
    );
}

#[test]
fn sqlite_ingest_detects_updates_and_deletions() {
    let (_temp, store, docs, collection) = setup_sqlite_store();

    fs::write(docs.join("a.md"), "# A\n\noriginal text").expect("write a");
    fs::write(docs.join("b.md"), "# B\n\noriginal text").expect("write b");

    let first = store.ingest_collections(false).expect("first ingest");
    assert_eq!(first.indexed_documents, 2);

    fs::write(docs.join("a.md"), "# A\n\nupdated text with more details").expect("update a");
    fs::remove_file(docs.join("b.md")).expect("remove b");

    let second = store.ingest_collections(false).expect("second ingest");
    assert!(second.updated_documents >= 1);
    assert_eq!(second.deactivated_documents, 1);

    let status = store.status().expect("status");
    let coll = status
        .collections
        .into_iter()
        .find(|c| c.name == collection)
        .expect("collection present");
    assert_eq!(coll.documents, 1);
}

#[test]
#[serial]
fn auto_store_defaults_to_sqlite_inside_git_repo() {
    let temp = tempfile::tempdir().expect("tempdir");
    fs::create_dir_all(temp.path().join(".git")).expect("create .git");

    let _guard = ProcessGuard::new(
        temp.path(),
        &[
            "QMD_BACKEND",
            "QMD_DATABASE_URL",
            "DATABASE_URL",
            "QMD_SQLITE_PATH",
        ],
    );

    let store = AutoQmdStore::open_default(Some("repo-default")).expect("auto store");
    assert_eq!(store.backend_kind(), QmdBackendKind::Sqlite);
}

#[test]
#[serial]
fn auto_store_defaults_to_existing_local_sqlite() {
    let temp = tempfile::tempdir().expect("tempdir");
    let local_db = temp.path().join(".litho").join("qmd").join("index.sqlite3");
    fs::create_dir_all(local_db.parent().expect("db parent")).expect("db dir");
    fs::write(&local_db, b"").expect("seed local sqlite file");

    let _guard = ProcessGuard::new(
        temp.path(),
        &[
            "QMD_BACKEND",
            "QMD_DATABASE_URL",
            "DATABASE_URL",
            "QMD_SQLITE_PATH",
        ],
    );

    let store = AutoQmdStore::open_default(None).expect("auto store");
    assert_eq!(store.backend_kind(), QmdBackendKind::Sqlite);
}
