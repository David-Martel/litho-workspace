use litho_qmd_core::{QmdService, QmdStore, SearchOptions};
use litho_qmd_llm::NoopLlmEngine;
use litho_qmd_storage::PostgresQmdStore;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::TempDir;

fn setup_store() -> Option<(TempDir, PostgresQmdStore, PathBuf, String)> {
    let db_url = std::env::var("QMD_TEST_DATABASE_URL")
        .or_else(|_| std::env::var("QMD_DATABASE_URL"))
        .ok()
        .or_else(repo_db_url)?;

    let temp = tempfile::tempdir().expect("tempdir");
    let docs = temp.path().join("docs");
    fs::create_dir_all(&docs).expect("create docs");

    let cfg_path = temp.path().join("index.yml");
    let salt = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let collection = format!("notes_{salt}");

    let docs_norm = docs.to_string_lossy().replace('\\', "/");
    let yaml =
        format!("collections:\n  {collection}:\n    path: '{docs_norm}'\n    pattern: '**/*.md'\n");
    fs::write(&cfg_path, yaml).expect("write config");

    let store = PostgresQmdStore::open_with_paths(db_url, &cfg_path).expect("open store");
    Some((temp, store, docs, collection))
}

#[derive(Debug, Deserialize)]
struct RepoCfg {
    #[serde(default)]
    database: RepoDbCfg,
}

#[derive(Debug, Deserialize, Default)]
struct RepoDbCfg {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    host: Option<String>,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    password: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

fn repo_db_url() -> Option<String> {
    let config_path = PathBuf::from("C:/codedev/litho-workspace/qmd.config.json");
    let raw = fs::read_to_string(config_path).ok()?;
    let cfg = serde_json::from_str::<RepoCfg>(&raw).ok()?;
    if let Some(url) = cfg.database.url {
        return Some(url);
    }
    let host = cfg.database.host.unwrap_or_else(|| "127.0.0.1".to_string());
    let port = cfg.database.port.unwrap_or(5432);
    let user = cfg.database.user.unwrap_or_else(|| "postgres".to_string());
    let pass = cfg
        .database
        .password
        .unwrap_or_else(|| "password".to_string());
    let name = cfg.database.name.unwrap_or_else(|| "qmd_index".to_string());
    Some(format!("postgresql://{user}:{pass}@{host}:{port}/{name}"))
}

#[test]
fn ingest_embed_and_search_pipeline() {
    let Some((_temp, store, docs, collection)) = setup_store() else {
        eprintln!(
            "skipping: set QMD_TEST_DATABASE_URL (or QMD_DATABASE_URL) for postgres integration tests"
        );
        return;
    };

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
fn ingest_detects_updates_and_deletions() {
    let Some((_temp, store, docs, collection)) = setup_store() else {
        eprintln!(
            "skipping: set QMD_TEST_DATABASE_URL (or QMD_DATABASE_URL) for postgres integration tests"
        );
        return;
    };

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
fn concurrent_store_bootstrap_is_idempotent() {
    let db_url = std::env::var("QMD_TEST_DATABASE_URL")
        .or_else(|_| std::env::var("QMD_DATABASE_URL"))
        .ok()
        .or_else(repo_db_url);
    let Some(db_url) = db_url else {
        eprintln!(
            "skipping: set QMD_TEST_DATABASE_URL (or QMD_DATABASE_URL) for postgres integration tests"
        );
        return;
    };

    let temp = tempfile::tempdir().expect("tempdir");
    let docs = temp.path().join("docs");
    fs::create_dir_all(&docs).expect("create docs");
    fs::write(docs.join("seed.md"), "# Seed\n\nbootstrap check").expect("write seed");

    let cfg_path = temp.path().join("index.yml");
    let salt = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let collection = format!("bootstrap_{salt}");
    let docs_norm = docs.to_string_lossy().replace('\\', "/");
    let yaml =
        format!("collections:\n  {collection}:\n    path: '{docs_norm}'\n    pattern: '**/*.md'\n");
    fs::write(&cfg_path, yaml).expect("write config");

    let mut handles = Vec::new();
    for _ in 0..4 {
        let db = db_url.clone();
        let cfg = cfg_path.clone();
        handles.push(thread::spawn(move || {
            let store = PostgresQmdStore::open_with_paths(db, &cfg).expect("open store");
            let _ = store.status().expect("status");
        }));
    }

    for handle in handles {
        handle.join().expect("join");
    }
}
