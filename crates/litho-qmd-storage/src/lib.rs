#[doc(hidden)]
pub mod fast_table;

use chrono::{DateTime, Utc};
use fast_table::{FastHashSet, FastHashTable};
use globset::Glob;
use ignore::WalkBuilder;
use litho_qmd_core::{
    CleanupReport, CollectionMutation, CollectionRecord, CollectionUpdateResult, ContextMutation,
    ContextRecord, ContextTarget, DocumentContent, DocumentRequest, EmbeddingReport, IndexStatus,
    IngestReport, MultiGetRequest, MultiGetResponse, QmdError, QmdResult, QmdStore, SearchHit,
    SearchOptions,
};
use native_tls::TlsConnector;
use postgres_native_tls::MakeTlsConnector;
use r2d2::{Pool, PooledConnection};
use r2d2_postgres::PostgresConnectionManager;
use r2d2_postgres::postgres::{Client, NoTls, config::SslMode};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const DEFAULT_INDEX_NAME: &str = "index";
const DEFAULT_PATTERN: &str = "**/*.md";
const NATIVE_VECTOR_DIM: usize = 256;
const NATIVE_VECTOR_MODEL: &str = "native-hash-v1";

pub struct PostgresQmdStore {
    database_url: String,
    config_path: PathBuf,
    pool: PgPool,
}

pub type PgQmdStore = PostgresQmdStore;
pub type SqliteQmdStore = PostgresQmdStore;

enum PgPool {
    NoTls(Pool<PostgresConnectionManager<NoTls>>),
    Tls(Pool<PostgresConnectionManager<MakeTlsConnector>>),
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RepoQmdConfig {
    #[serde(default)]
    database: RepoDatabaseConfig,
    #[serde(default)]
    paths: RepoPathsConfig,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RepoPathsConfig {
    #[serde(default)]
    collections_config_path: Option<String>,
    #[serde(default)]
    search_telemetry_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RepoDatabaseConfig {
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
    #[serde(default)]
    sslmode: Option<String>,
    #[serde(default)]
    admin_db: Option<String>,
    #[serde(default)]
    bootstrap: Option<bool>,
    #[serde(default)]
    pool_max: Option<u32>,
    #[serde(default)]
    pool_min_idle: Option<u32>,
    #[serde(default)]
    pool_connect_timeout_ms: Option<u64>,
    #[serde(default)]
    pool_idle_timeout_ms: Option<u64>,
    #[serde(default)]
    pool_max_lifetime_ms: Option<u64>,
    #[serde(default)]
    pool_test_on_check_out: Option<bool>,
    #[serde(default)]
    db_connect_timeout_ms: Option<u64>,
    #[serde(default)]
    allow_tls: Option<bool>,
    #[serde(default)]
    allow_insecure_tls: Option<bool>,
}

#[derive(Debug, Clone)]
struct RuntimeDbSettings {
    database_url: String,
    admin_db: String,
    bootstrap: bool,
    pool_max: u32,
    pool_min_idle: u32,
    pool_connect_timeout_ms: u64,
    pool_idle_timeout_ms: u64,
    pool_max_lifetime_ms: u64,
    pool_test_on_check_out: bool,
    db_connect_timeout_ms: u64,
    allow_tls: bool,
    allow_insecure_tls: bool,
}

impl PostgresQmdStore {
    pub fn open_default(index_name: Option<&str>) -> QmdResult<Self> {
        let index = index_name.unwrap_or(DEFAULT_INDEX_NAME);
        let runtime = resolve_runtime_db_settings(index);
        let config_path = resolve_collection_config_path(index)?;
        Self::open_with_runtime(runtime.database_url.clone(), config_path, runtime)
    }

    pub fn open_with_paths(
        database_url: impl Into<String>,
        config_path: impl Into<PathBuf>,
    ) -> QmdResult<Self> {
        let database_url = database_url.into();
        let config_path = config_path.into();
        let runtime = default_runtime_db_settings(database_url.clone());
        Self::open_with_runtime(database_url, config_path, runtime)
    }

    fn open_with_runtime(
        database_url: String,
        config_path: PathBuf,
        runtime: RuntimeDbSettings,
    ) -> QmdResult<Self> {
        let database_url = database_url;
        let config_path = config_path;

        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }

        let mut config = parse_postgres_config(&database_url)?;
        config.connect_timeout(Duration::from_millis(runtime.db_connect_timeout_ms));
        if !dsn_has_password(&database_url) {
            let password = runtime_password().or_else(|| {
                repo_qmd_config()
                    .database
                    .password
                    .as_ref()
                    .map(ToOwned::to_owned)
            });
            if let Some(password) = password
                && !password.is_empty()
            {
                config.password(password);
            }
        }

        if runtime.bootstrap {
            bootstrap_database_if_needed(
                &config,
                &runtime.admin_db,
                runtime.allow_tls,
                runtime.allow_insecure_tls,
            )?;
        }

        let mut no_tls_config = config.clone();
        no_tls_config.ssl_mode(SslMode::Disable);

        let pool = match no_tls_config.connect(NoTls) {
            Ok(_) => {
                let manager = PostgresConnectionManager::new(no_tls_config, NoTls);
                let pool = Pool::builder()
                    .max_size(runtime.pool_max.max(4))
                    .min_idle(Some(runtime.pool_min_idle.min(runtime.pool_max.max(4))))
                    .connection_timeout(Duration::from_millis(runtime.pool_connect_timeout_ms))
                    .idle_timeout(Some(Duration::from_millis(runtime.pool_idle_timeout_ms)))
                    .max_lifetime(Some(Duration::from_millis(runtime.pool_max_lifetime_ms)))
                    .test_on_check_out(runtime.pool_test_on_check_out)
                    .build(manager)
                    .map_err(pool_error)?;
                {
                    let mut conn = pool.get().map_err(pool_error)?;
                    initialize_schema(&mut conn)?;
                }
                PgPool::NoTls(pool)
            }
            Err(no_tls_err) => {
                if !runtime.allow_tls {
                    return Err(QmdError::Internal(format!(
                        "postgres connectivity check failed (NoTLS) for QMD_DATABASE_URL: {no_tls_err}"
                    )));
                }

                let mut tls_builder = TlsConnector::builder();
                if runtime.allow_insecure_tls {
                    tls_builder.danger_accept_invalid_certs(true);
                }
                let tls_connector = tls_builder.build().map_err(|e| {
                    QmdError::Internal(format!("failed to build TLS connector: {e}"))
                })?;

                let tls_probe = config.connect(MakeTlsConnector::new(tls_connector.clone()));
                if let Err(tls_err) = tls_probe {
                    let hint = missing_password_hint(&database_url);
                    return Err(QmdError::Internal(format!(
                        "postgres NoTLS connectivity failed: {no_tls_err}; TLS fallback failed: {tls_err}{hint}"
                    )));
                }

                let manager =
                    PostgresConnectionManager::new(config, MakeTlsConnector::new(tls_connector));
                let pool = Pool::builder()
                    .max_size(runtime.pool_max.max(4))
                    .min_idle(Some(runtime.pool_min_idle.min(runtime.pool_max.max(4))))
                    .connection_timeout(Duration::from_millis(runtime.pool_connect_timeout_ms))
                    .idle_timeout(Some(Duration::from_millis(runtime.pool_idle_timeout_ms)))
                    .max_lifetime(Some(Duration::from_millis(runtime.pool_max_lifetime_ms)))
                    .test_on_check_out(runtime.pool_test_on_check_out)
                    .build(manager)
                    .map_err(pool_error)?;
                {
                    let mut conn = pool.get().map_err(pool_error)?;
                    initialize_schema(&mut conn)?;
                }
                PgPool::Tls(pool)
            }
        };

        Ok(Self {
            database_url,
            config_path,
            pool,
        })
    }

    pub fn database_url(&self) -> &str {
        &self.database_url
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    fn with_conn<T>(&self, f: impl FnOnce(&mut Client) -> QmdResult<T>) -> QmdResult<T> {
        match &self.pool {
            PgPool::NoTls(pool) => {
                let mut conn: PooledConnection<_> = pool.get().map_err(pool_error)?;
                f(&mut conn)
            }
            PgPool::Tls(pool) => {
                let mut conn: PooledConnection<_> = pool.get().map_err(pool_error)?;
                f(&mut conn)
            }
        }
    }

    fn load_config(&self) -> QmdResult<ConfigFile> {
        if !self.config_path.exists() {
            return Ok(ConfigFile::default());
        }

        let content = fs::read_to_string(&self.config_path).map_err(io_error)?;
        let parsed: ConfigFile = serde_yaml::from_str(&content)
            .map_err(|e| QmdError::Internal(format!("failed to parse config: {e}")))?;
        Ok(parsed)
    }

    fn save_config(&self, config: &ConfigFile) -> QmdResult<()> {
        let yaml = serde_yaml::to_string(config)
            .map_err(|e| QmdError::Internal(format!("failed to serialize config: {e}")))?;
        fs::write(&self.config_path, yaml).map_err(io_error)
    }

    fn resolve_document(&self, file: &str) -> QmdResult<StoredDocument> {
        let (lookup, parsed_from_line) = parse_file_lookup(file);
        let doc = self.with_conn(|conn| {
            if let Some(docid) = lookup.strip_prefix('#') {
                let id = docid.parse::<i64>().map_err(|_| {
                    QmdError::InvalidRequest(format!("docid must be numeric, got '{docid}'"))
                })?;
                return get_document_by_id(conn, id);
            }

            if let Some((collection, path)) = split_collection_path(lookup) {
                let exact = get_document_by_collection_path(conn, collection, path)?;
                if exact.is_some() {
                    return Ok(exact);
                }
            }

            get_document_by_suffix(conn, lookup)
        })?;

        let mut doc =
            doc.ok_or_else(|| QmdError::NotFound(format!("document not found: {}", lookup)))?;
        if parsed_from_line.is_some() {
            doc.default_from_line = parsed_from_line;
        }
        Ok(doc)
    }

    fn context_for_path(config: &ConfigFile, display_path: &str) -> Option<String> {
        let Some((collection_name, rel)) = split_collection_path(display_path) else {
            return config.global_context.clone();
        };

        let Some(collection) = config.collections.get(collection_name) else {
            return config.global_context.clone();
        };

        if collection.context.is_empty() {
            return config.global_context.clone();
        }

        let normalized_rel = normalize_context_path(rel);
        let mut best: Option<(usize, String)> = None;
        for (prefix, text) in &collection.context {
            let normalized_prefix = normalize_context_path(prefix);
            if normalized_rel == normalized_prefix
                || normalized_rel.starts_with(&(normalized_prefix.clone() + "/"))
            {
                let len = normalized_prefix.len();
                let should_replace = best
                    .as_ref()
                    .map(|(best_len, _)| len > *best_len)
                    .unwrap_or(true);
                if should_replace {
                    best = Some((len, text.clone()));
                }
            }
        }

        best.map(|(_, text)| text)
            .or_else(|| config.global_context.clone())
    }
}

impl QmdStore for PostgresQmdStore {
    fn status(&self) -> QmdResult<IndexStatus> {
        let config = self.load_config()?;
        let (total_documents, needs_embedding, has_vector_index, counts, updates) =
            self.with_conn(|conn| {
                let total_documents: i64 = conn
                    .query_one("SELECT COUNT(*) FROM documents WHERE active = TRUE", &[])
                    .map_err(pg_error)?
                    .get(0);

                let needs_embedding: i64 = conn
                    .query_one(
                        r#"
                        SELECT COUNT(*)
                        FROM documents d
                        WHERE d.active = TRUE
                        AND NOT EXISTS (
                            SELECT 1 FROM content_vectors_native cv WHERE cv.hash = d.hash
                        )
                        "#,
                        &[],
                    )
                    .map_err(pg_error)?
                    .get(0);

                let has_vector_index: bool = conn
                    .query_one(
                        "SELECT to_regclass('public.content_vectors_native') IS NOT NULL",
                        &[],
                    )
                    .map_err(pg_error)?
                    .get(0);

                let mut counts = BTreeMap::new();
                for row in conn
                    .query(
                        "SELECT collection, COUNT(*) FROM documents WHERE active = TRUE GROUP BY collection",
                        &[],
                    )
                    .map_err(pg_error)?
                {
                    let name: String = row.get(0);
                    let count: i64 = row.get(1);
                    counts.insert(name, count as usize);
                }

                let mut updates = BTreeMap::new();
                for row in conn
                    .query(
                        "SELECT collection, MAX(modified_at) FROM documents WHERE active = TRUE GROUP BY collection",
                        &[],
                    )
                    .map_err(pg_error)?
                {
                    let name: String = row.get(0);
                    let modified: Option<DateTime<Utc>> = row.get(1);
                    updates.insert(
                        name,
                        modified.map(|v| v.to_rfc3339()).unwrap_or_default(),
                    );
                }

                Ok((
                    total_documents as usize,
                    needs_embedding as usize,
                    has_vector_index,
                    counts,
                    updates,
                ))
            })?;

        let collections = config
            .collections
            .iter()
            .map(|(name, item)| CollectionRecord {
                name: name.clone(),
                path: item.path.clone(),
                pattern: item.pattern.clone(),
                documents: counts.get(name).copied().unwrap_or(0),
                last_updated: updates.get(name).cloned(),
            })
            .collect();

        Ok(IndexStatus {
            total_documents,
            needs_embedding,
            has_vector_index,
            collections,
        })
    }

    fn search_bm25(&self, query: &str, options: &SearchOptions) -> QmdResult<Vec<SearchHit>> {
        let started = Instant::now();
        let config = self.load_config()?;
        let result = self.with_conn(|conn| {
            let strategies = build_query_variants(query);
            let query_terms = tokenize(query);
            let mut fused = BTreeMap::<String, SearchHit>::new();

            for (strategy_weight, q) in strategies {
                let rows = run_fts_query(
                    conn,
                    &q,
                    options.limit.saturating_mul(4),
                    options.collection.as_deref(),
                )?;
                for row in rows {
                    let file = format!("{}/{}", row.collection, row.path);
                    let overlap = term_overlap_ratio(&query_terms, &row.title, &row.body);
                    let rank = row.rank.max(0.0) as f32;
                    let score = (0.72 * rank.min(1.0) + 0.28 * overlap)
                        * strategy_weight
                        * path_quality_multiplier(&file);
                    let snippet = extract_snippet_multi(&row.body, &query_terms, 360);
                    let context = Self::context_for_path(&config, &file);
                    let hit = SearchHit {
                        docid: format!("#{}", row.id),
                        file: file.clone(),
                        title: row.title,
                        score,
                        context,
                        snippet,
                    };

                    match fused.get(&row.hash) {
                        Some(existing) if existing.score >= hit.score => {}
                        _ => {
                            fused.insert(row.hash, hit);
                        }
                    }
                }
            }

            if fused.is_empty() {
                let fallback_rows = run_path_title_fallback(
                    conn,
                    query,
                    options.limit,
                    options.collection.as_deref(),
                )?;
                for row in fallback_rows {
                    let file = format!("{}/{}", row.collection, row.path);
                    let context = Self::context_for_path(&config, &file);
                    let hit = SearchHit {
                        docid: format!("#{}", row.id),
                        file: file.clone(),
                        title: row.title,
                        score: 0.15 * path_quality_multiplier(&file),
                        context,
                        snippet: extract_snippet(&row.body, query, 360),
                    };
                    match fused.get(&row.hash) {
                        Some(existing) if existing.score >= hit.score => {}
                        _ => {
                            fused.insert(row.hash, hit);
                        }
                    }
                }
            }

            let mut out = fused.into_values().collect::<Vec<_>>();
            out.retain(|h| h.score >= options.min_score);
            out.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.file.cmp(&b.file)));
            out.truncate(options.limit);
            Ok(out)
        });

        if let Ok(ref hits) = result {
            emit_search_telemetry("bm25", query, options.limit, hits.len(), started.elapsed());
        }
        result
    }

    fn search_vector(&self, query: &str, options: &SearchOptions) -> QmdResult<Vec<SearchHit>> {
        let started = Instant::now();
        let config = self.load_config()?;
        let q_vec = quantize_embedding(&embed_text_native(query, NATIVE_VECTOR_DIM));
        let result = self.with_conn(|conn| {
            let table_exists: bool = conn
                .query_one(
                    "SELECT to_regclass('public.content_vectors_native') IS NOT NULL",
                    &[],
                )
                .map_err(pg_error)?
                .get(0);
            if !table_exists {
                return Ok(Vec::new());
            }

            let lsh = compute_lsh_buckets(&q_vec, 8);
            let mut candidates = run_vector_prefilter(conn, &lsh, options.collection.as_deref())?;
            if candidates.is_empty() {
                candidates = run_vector_scan_all(conn, options.collection.as_deref())?;
            }

            let query_terms = tokenize(query);
            let mut scored = BTreeMap::<String, SearchHit>::new();
            for row in candidates {
                let d_vec = decode_qvec(&row.qvec_blob)?;
                if d_vec.len() != q_vec.len() {
                    continue;
                }
                let semantic = cosine_similarity_qvec(&q_vec, &d_vec);
                if semantic <= 0.0 {
                    continue;
                }
                let lexical = term_overlap_ratio(&query_terms, &row.title, &row.body);
                let file = format!("{}/{}", row.collection, row.path);
                let score = (0.74 * semantic + 0.26 * lexical) * path_quality_multiplier(&file);
                let context = Self::context_for_path(&config, &file);
                let hit = SearchHit {
                    docid: format!("#{}", row.id),
                    file,
                    title: row.title,
                    score,
                    context,
                    snippet: extract_snippet_multi(&row.body, &query_terms, 360),
                };
                match scored.get(&row.hash) {
                    Some(existing) if existing.score >= hit.score => {}
                    _ => {
                        scored.insert(row.hash, hit);
                    }
                }
            }

            let mut scored = scored.into_values().collect::<Vec<_>>();
            scored.retain(|h| h.score >= options.min_score);
            scored.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.file.cmp(&b.file)));
            scored.truncate(options.limit);
            Ok(scored)
        });

        if let Ok(ref hits) = result {
            emit_search_telemetry(
                "vector",
                query,
                options.limit,
                hits.len(),
                started.elapsed(),
            );
        }
        result
    }

    fn get_document(&self, request: &DocumentRequest) -> QmdResult<DocumentContent> {
        let config = self.load_config()?;
        let doc = self.resolve_document(&request.file)?;
        let from_line = request.from_line.or(doc.default_from_line);
        let text = slice_text(
            &doc.body,
            from_line,
            request.max_lines,
            request.line_numbers,
        );
        let display_path = format!("{}/{}", doc.collection, doc.path);
        let context = Self::context_for_path(&config, &display_path);

        Ok(DocumentContent {
            uri: format!("qmd://{}", display_path),
            name: display_path,
            title: doc.title,
            text,
            context,
        })
    }

    fn multi_get(&self, request: &MultiGetRequest) -> QmdResult<MultiGetResponse> {
        let candidates = if request.pattern.contains(',') {
            request
                .pattern
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        } else {
            let matcher = Glob::new(&request.pattern)
                .map_err(|e| QmdError::InvalidRequest(format!("invalid glob pattern: {e}")))?
                .compile_matcher();
            self.list_files(None)?
                .into_iter()
                .filter(|p| matcher.is_match(p))
                .collect::<Vec<_>>()
        };

        let mut seen = HashSet::new();
        let mut docs = Vec::new();
        let mut errors = Vec::new();

        for candidate in candidates {
            if !seen.insert(candidate.clone()) {
                continue;
            }

            match self.resolve_document(&candidate) {
                Ok(doc) => {
                    if doc.body.len() > request.max_bytes {
                        errors.push(format!(
                            "skipped {}/{}: file size {} exceeds max-bytes {}",
                            doc.collection,
                            doc.path,
                            doc.body.len(),
                            request.max_bytes
                        ));
                        continue;
                    }

                    let text = slice_text(&doc.body, None, request.max_lines, request.line_numbers);
                    docs.push(DocumentContent {
                        uri: format!("qmd://{}/{}", doc.collection, doc.path),
                        name: format!("{}/{}", doc.collection, doc.path),
                        title: doc.title,
                        text,
                        context: None,
                    });
                }
                Err(err) => errors.push(err.to_string()),
            }
        }

        Ok(MultiGetResponse {
            documents: docs,
            errors,
        })
    }

    fn list_files(&self, prefix: Option<&str>) -> QmdResult<Vec<String>> {
        self.with_conn(|conn| {
            let rows = if let Some(prefix) = prefix {
                let like = format!("{prefix}%");
                conn.query(
                    r#"
                    SELECT collection || '/' || path AS display_path
                    FROM documents
                    WHERE active = TRUE
                    AND (collection || '/' || path) ILIKE $1
                    ORDER BY display_path
                    "#,
                    &[&like],
                )
                .map_err(pg_error)?
            } else {
                conn.query(
                    r#"
                    SELECT collection || '/' || path AS display_path
                    FROM documents
                    WHERE active = TRUE
                    ORDER BY display_path
                    "#,
                    &[],
                )
                .map_err(pg_error)?
            };

            Ok(rows.into_iter().map(|r| r.get::<_, String>(0)).collect())
        })
    }

    fn list_collections(&self) -> QmdResult<Vec<CollectionRecord>> {
        self.status().map(|s| s.collections)
    }

    fn add_collection(&self, input: &CollectionMutation) -> QmdResult<()> {
        let mut config = self.load_config()?;
        config.collections.insert(
            input.name.clone(),
            CollectionConfig {
                path: input.path.clone(),
                pattern: input.pattern.clone(),
                context: BTreeMap::new(),
                update: None,
            },
        );
        self.save_config(&config)
    }

    fn remove_collection(&self, name: &str) -> QmdResult<bool> {
        let mut config = self.load_config()?;
        let existed = config.collections.remove(name).is_some();
        self.save_config(&config)?;
        Ok(existed)
    }

    fn rename_collection(&self, old_name: &str, new_name: &str) -> QmdResult<()> {
        let mut config = self.load_config()?;
        if config.collections.contains_key(new_name) {
            return Err(QmdError::InvalidRequest(format!(
                "collection already exists: {new_name}"
            )));
        }
        let old = config
            .collections
            .remove(old_name)
            .ok_or_else(|| QmdError::NotFound(format!("collection not found: {old_name}")))?;
        config.collections.insert(new_name.to_string(), old);
        self.save_config(&config)
    }

    fn run_collection_updates(&self) -> QmdResult<Vec<CollectionUpdateResult>> {
        let config = self.load_config()?;
        let mut results = Vec::new();

        for (name, collection) in config.collections {
            let Some(command) = collection.update else {
                continue;
            };

            let mut cmd = if cfg!(windows) {
                let mut c = Command::new("cmd");
                c.arg("/C").arg(&command);
                c
            } else {
                let mut c = Command::new("sh");
                c.arg("-lc").arg(&command);
                c
            };
            cmd.current_dir(&collection.path);

            match cmd.output() {
                Ok(output) => results.push(CollectionUpdateResult {
                    collection: name,
                    command,
                    success: output.status.success(),
                    exit_code: output.status.code(),
                    stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
                }),
                Err(err) => results.push(CollectionUpdateResult {
                    collection: name,
                    command,
                    success: false,
                    exit_code: None,
                    stderr: err.to_string(),
                }),
            }
        }

        Ok(results)
    }

    fn ingest_collections(&self, force: bool) -> QmdResult<IngestReport> {
        let started = Instant::now();
        let config = self.load_config()?;
        let mut report = IngestReport::default();

        for (collection_name, collection) in config.collections {
            let root = PathBuf::from(&collection.path);
            if !root.exists() {
                continue;
            }

            let matcher = Glob::new(&collection.pattern)
                .map_err(|e| {
                    QmdError::InvalidRequest(format!("invalid glob for {collection_name}: {e}"))
                })?
                .compile_matcher();

            #[derive(Debug)]
            struct Candidate {
                rel_norm: String,
                hash: String,
                title: String,
                modified_at: DateTime<Utc>,
                content: String,
            }

            let mut candidates: Vec<Candidate> = Vec::new();
            let mut seen_paths = FastHashSet::with_capacity(1024);
            let walker = WalkBuilder::new(&root)
                .hidden(false)
                .follow_links(false)
                .standard_filters(false)
                .build();

            for entry in walker {
                let entry = match entry {
                    Ok(v) => v,
                    Err(_) => {
                        report.failed_files += 1;
                        continue;
                    }
                };
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }

                let Ok(relative) = path.strip_prefix(&root) else {
                    continue;
                };
                let rel_norm = relative.to_string_lossy().replace('\\', "/");
                if !matcher.is_match(rel_norm.as_str()) {
                    continue;
                }
                report.scanned_files += 1;
                seen_paths.insert(rel_norm.clone());

                let content_bytes = match fs::read(path) {
                    Ok(bytes) => bytes,
                    Err(_) => {
                        report.failed_files += 1;
                        continue;
                    }
                };
                let content = String::from_utf8_lossy(&content_bytes).to_string();
                let hash = blake3::hash(content.as_bytes()).to_hex().to_string();
                let title = derive_title(&rel_norm, &content);
                let modified_at = filesystem_modified_utc(path);
                candidates.push(Candidate {
                    rel_norm,
                    hash,
                    title,
                    modified_at,
                    content,
                });
            }

            self.with_conn(|conn| {
                let mut tx = conn.transaction().map_err(pg_error)?;

                let mut existing_map = FastHashTable::<String, String>::with_capacity(
                    candidates.len().saturating_mul(2).max(64),
                );
                for row in tx
                    .query(
                        "SELECT path, hash FROM documents WHERE collection = $1",
                        &[&collection_name],
                    )
                    .map_err(pg_error)?
                {
                    let path: String = row.get(0);
                    let hash: String = row.get(1);
                    let _ = existing_map.insert(path, hash);
                }

                let now = Utc::now();
                for candidate in &candidates {
                    tx.execute(
                        "INSERT INTO content(hash, doc, created_at) VALUES($1, $2, $3) ON CONFLICT(hash) DO NOTHING",
                        &[&candidate.hash, &candidate.content, &now],
                    )
                    .map_err(pg_error)?;

                    match existing_map.get(&candidate.rel_norm) {
                        Some(existing_hash) if existing_hash == &candidate.hash && !force => {
                            tx.execute(
                                "UPDATE documents SET active = TRUE, modified_at = $3 WHERE collection = $1 AND path = $2",
                                &[&collection_name, &candidate.rel_norm, &candidate.modified_at],
                            )
                            .map_err(pg_error)?;
                            report.unchanged_documents += 1;
                        }
                        Some(_) => {
                            tx.execute(
                                r#"
                                UPDATE documents
                                SET hash = $3,
                                    title = $4,
                                    active = TRUE,
                                    modified_at = $5,
                                    search = to_tsvector('simple', concat_ws(' ', $2::text, $4::text, $6::text))
                                WHERE collection = $1 AND path = $2
                                "#,
                                &[
                                    &collection_name,
                                    &candidate.rel_norm,
                                    &candidate.hash,
                                    &candidate.title,
                                    &candidate.modified_at,
                                    &candidate.content,
                                ],
                            )
                            .map_err(pg_error)?;
                            report.updated_documents += 1;
                        }
                        None => {
                            tx.execute(
                                r#"
                                INSERT INTO documents(
                                    collection,
                                    path,
                                    title,
                                    hash,
                                    created_at,
                                    modified_at,
                                    active,
                                    search
                                )
                                VALUES(
                                    $1,
                                    $2,
                                    $3,
                                    $4,
                                    $5,
                                    $6,
                                    TRUE,
                                    to_tsvector('simple', concat_ws(' ', $2::text, $3::text, $7::text))
                                )
                                "#,
                                &[
                                    &collection_name,
                                    &candidate.rel_norm,
                                    &candidate.title,
                                    &candidate.hash,
                                    &now,
                                    &candidate.modified_at,
                                    &candidate.content,
                                ],
                            )
                            .map_err(pg_error)?;
                            report.indexed_documents += 1;
                        }
                    }
                }

                for row in tx
                    .query(
                        "SELECT path FROM documents WHERE collection = $1 AND active = TRUE",
                        &[&collection_name],
                    )
                    .map_err(pg_error)?
                {
                    let old: String = row.get(0);
                    if !seen_paths.contains(&old) {
                        tx.execute(
                            "UPDATE documents SET active = FALSE WHERE collection = $1 AND path = $2 AND active = TRUE",
                            &[&collection_name, &old],
                        )
                        .map_err(pg_error)?;
                        report.deactivated_documents += 1;
                    }
                }

                tx.commit().map_err(pg_error)?;
                Ok(())
            })?;
        }

        report.duration_ms = started.elapsed().as_millis();
        Ok(report)
    }

    fn embed_native(&self, force: bool) -> QmdResult<EmbeddingReport> {
        let started = Instant::now();
        let mut report = EmbeddingReport {
            model: NATIVE_VECTOR_MODEL.to_string(),
            dimension: NATIVE_VECTOR_DIM,
            ..EmbeddingReport::default()
        };

        let docs = self.with_conn(|conn| {
            let mut out = Vec::new();
            for row in conn
                .query(
                    r#"
                    SELECT DISTINCT d.hash, c.doc
                    FROM documents d
                    JOIN content c ON c.hash = d.hash
                    WHERE d.active = TRUE
                    "#,
                    &[],
                )
                .map_err(pg_error)?
            {
                let hash: String = row.get(0);
                let doc: String = row.get(1);
                out.push((hash, doc));
            }
            Ok(out)
        })?;

        let existing = self.with_conn(|conn| {
            let mut out = HashSet::new();
            for row in conn
                .query("SELECT hash FROM content_vectors_native", &[])
                .map_err(pg_error)?
            {
                out.insert(row.get::<_, String>(0));
            }
            Ok(out)
        })?;

        let total_docs = docs.len();
        let to_embed = docs
            .into_iter()
            .filter(|(hash, _)| force || !existing.contains(hash))
            .collect::<Vec<_>>();
        report.skipped = total_docs.saturating_sub(to_embed.len());

        self.with_conn(|conn| {
            let mut tx = conn.transaction().map_err(pg_error)?;
            for (hash, text) in to_embed {
                let vec = embed_text_native(&text, NATIVE_VECTOR_DIM);
                let qvec = quantize_embedding(&vec);
                let lsh = compute_lsh_buckets(&qvec, 8);

                tx.execute(
                    r#"
                    INSERT INTO content_vectors_native(hash, dim, qvec, model, embedded_at)
                    VALUES($1, $2, $3, $4, $5)
                    ON CONFLICT(hash) DO UPDATE SET
                      dim = excluded.dim,
                      qvec = excluded.qvec,
                      model = excluded.model,
                      embedded_at = excluded.embedded_at
                    "#,
                    &[
                        &hash,
                        &(NATIVE_VECTOR_DIM as i32),
                        &encode_qvec(&qvec),
                        &NATIVE_VECTOR_MODEL,
                        &Utc::now(),
                    ],
                )
                .map_err(pg_error)?;

                tx.execute("DELETE FROM content_vectors_lsh WHERE hash = $1", &[&hash])
                    .map_err(pg_error)?;
                for (band, bucket) in &lsh {
                    tx.execute(
                        "INSERT INTO content_vectors_lsh(hash, band, bucket) VALUES($1, $2, $3) ON CONFLICT DO NOTHING",
                        &[&hash, &(*band as i32), bucket],
                    )
                    .map_err(pg_error)?;
                }
                report.embedded += 1;
            }

            tx.commit().map_err(pg_error)?;
            Ok(())
        })?;

        report.duration_ms = started.elapsed().as_millis();
        Ok(report)
    }

    fn list_contexts(&self) -> QmdResult<Vec<ContextRecord>> {
        let config = self.load_config()?;
        let mut out = Vec::new();

        if let Some(global) = config.global_context {
            out.push(ContextRecord {
                scope: "global".to_string(),
                path: "/".to_string(),
                text: global,
            });
        }

        for (collection_name, collection) in config.collections {
            for (path, text) in collection.context {
                out.push(ContextRecord {
                    scope: collection_name.clone(),
                    path,
                    text,
                });
            }
        }

        out.sort_by(|a, b| a.scope.cmp(&b.scope).then(a.path.cmp(&b.path)));
        Ok(out)
    }

    fn add_context(&self, input: &ContextMutation) -> QmdResult<()> {
        let mut config = self.load_config()?;
        match &input.target {
            ContextTarget::Global => {
                config.global_context = Some(input.text.clone());
            }
            ContextTarget::CollectionPath { collection, path } => {
                let item = config.collections.get_mut(collection).ok_or_else(|| {
                    QmdError::NotFound(format!("collection not found: {collection}"))
                })?;
                item.context
                    .insert(normalize_context_path(path), input.text.clone());
            }
        }
        self.save_config(&config)
    }

    fn remove_context(&self, target: &ContextTarget) -> QmdResult<bool> {
        let mut config = self.load_config()?;
        let removed = match target {
            ContextTarget::Global => config.global_context.take().is_some(),
            ContextTarget::CollectionPath { collection, path } => {
                if let Some(item) = config.collections.get_mut(collection) {
                    item.context.remove(&normalize_context_path(path)).is_some()
                } else {
                    false
                }
            }
        };
        self.save_config(&config)?;
        Ok(removed)
    }

    fn cleanup(&self) -> QmdResult<CleanupReport> {
        let report = self.with_conn(|conn| {
            let llm_cache_rows = conn.execute("DELETE FROM llm_cache", &[]).map_err(pg_error)?;
            let inactive_documents = conn
                .execute("DELETE FROM documents WHERE active = FALSE", &[])
                .map_err(pg_error)?;
            let orphaned_content = conn
                .execute(
                    "DELETE FROM content WHERE hash NOT IN (SELECT DISTINCT hash FROM documents)",
                    &[],
                )
                .map_err(pg_error)?;
            let orphaned_vectors = conn
                .execute(
                    "DELETE FROM content_vectors_native WHERE hash NOT IN (SELECT DISTINCT hash FROM content)",
                    &[],
                )
                .map_err(pg_error)?;
            conn.execute(
                "DELETE FROM content_vectors_lsh WHERE hash NOT IN (SELECT DISTINCT hash FROM content_vectors_native)",
                &[],
            )
            .map_err(pg_error)?;

            Ok(CleanupReport {
                llm_cache_rows: llm_cache_rows as usize,
                inactive_documents: inactive_documents as usize,
                orphaned_content: orphaned_content as usize,
                orphaned_vectors: orphaned_vectors as usize,
            })
        })?;

        let _ = self.with_conn(|conn| {
            conn.batch_execute("VACUUM (ANALYZE)").map_err(pg_error)?;
            Ok(())
        });

        Ok(report)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ConfigFile {
    #[serde(default)]
    global_context: Option<String>,
    #[serde(default)]
    collections: BTreeMap<String, CollectionConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CollectionConfig {
    path: String,
    #[serde(default = "default_pattern")]
    pattern: String,
    #[serde(default)]
    context: BTreeMap<String, String>,
    #[serde(default)]
    update: Option<String>,
}

impl Default for CollectionConfig {
    fn default() -> Self {
        Self {
            path: String::new(),
            pattern: default_pattern(),
            context: BTreeMap::new(),
            update: None,
        }
    }
}

#[derive(Debug, Clone)]
struct StoredDocument {
    collection: String,
    path: String,
    title: String,
    body: String,
    default_from_line: Option<usize>,
}

fn initialize_schema(conn: &mut Client) -> QmdResult<()> {
    // Prevent concurrent schema bootstrap races across parallel test/process runs.
    const SCHEMA_INIT_LOCK_KEY: i64 = 0x6C6974686F514D44;
    conn.query_one("SELECT pg_advisory_lock($1)", &[&SCHEMA_INIT_LOCK_KEY])
        .map_err(pg_error)?;

    let schema_result = conn.batch_execute(
        r#"
        CREATE TABLE IF NOT EXISTS content (
            hash TEXT PRIMARY KEY,
            doc TEXT NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );

        CREATE TABLE IF NOT EXISTS documents (
            id BIGSERIAL PRIMARY KEY,
            collection TEXT NOT NULL,
            path TEXT NOT NULL,
            title TEXT NOT NULL,
            hash TEXT NOT NULL REFERENCES content(hash) ON DELETE CASCADE,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            active BOOLEAN NOT NULL DEFAULT TRUE,
            search tsvector NOT NULL DEFAULT ''::tsvector,
            UNIQUE(collection, path)
        );

        CREATE INDEX IF NOT EXISTS idx_documents_collection ON documents(collection, active);
        CREATE INDEX IF NOT EXISTS idx_documents_hash ON documents(hash);
        CREATE INDEX IF NOT EXISTS idx_documents_path ON documents(path, active);
        CREATE INDEX IF NOT EXISTS idx_documents_display_path ON documents((collection || '/' || path));
        CREATE INDEX IF NOT EXISTS idx_documents_active_lookup ON documents(collection, path) WHERE active = TRUE;
        CREATE INDEX IF NOT EXISTS idx_documents_active_hash_collection ON documents(hash, collection) WHERE active = TRUE;
        CREATE INDEX IF NOT EXISTS idx_documents_search ON documents USING GIN(search);

        CREATE TABLE IF NOT EXISTS llm_cache (
            hash TEXT PRIMARY KEY,
            result TEXT NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );

        CREATE TABLE IF NOT EXISTS content_vectors_native (
            hash TEXT PRIMARY KEY,
            dim INTEGER NOT NULL,
            qvec BYTEA NOT NULL,
            model TEXT NOT NULL,
            embedded_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        CREATE INDEX IF NOT EXISTS idx_content_vectors_native_model ON content_vectors_native(model);

        CREATE TABLE IF NOT EXISTS content_vectors_lsh (
            hash TEXT NOT NULL,
            band INTEGER NOT NULL,
            bucket BIGINT NOT NULL,
            PRIMARY KEY(hash, band, bucket)
        );
        CREATE INDEX IF NOT EXISTS idx_content_vectors_lsh_lookup ON content_vectors_lsh(band, bucket);
        "#,
    )
    .map_err(pg_error);

    let unlock_result = conn
        .query_one("SELECT pg_advisory_unlock($1)", &[&SCHEMA_INIT_LOCK_KEY])
        .map_err(pg_error);

    match (schema_result, unlock_result) {
        (Err(schema_err), _) => Err(schema_err),
        (Ok(_), Err(unlock_err)) => Err(unlock_err),
        (Ok(_), Ok(_)) => Ok(()),
    }
}

fn get_document_by_id(conn: &mut Client, id: i64) -> QmdResult<Option<StoredDocument>> {
    let row = conn
        .query_opt(
            r#"
            SELECT d.collection, d.path, d.title, c.doc
            FROM documents d
            JOIN content c ON c.hash = d.hash
            WHERE d.id = $1 AND d.active = TRUE
            "#,
            &[&id],
        )
        .map_err(pg_error)?;

    Ok(row.map(|r| StoredDocument {
        collection: r.get(0),
        path: r.get(1),
        title: r.get(2),
        body: r.get(3),
        default_from_line: None,
    }))
}

fn get_document_by_collection_path(
    conn: &mut Client,
    collection: &str,
    path: &str,
) -> QmdResult<Option<StoredDocument>> {
    let row = conn
        .query_opt(
            r#"
            SELECT d.collection, d.path, d.title, c.doc
            FROM documents d
            JOIN content c ON c.hash = d.hash
            WHERE d.collection = $1 AND d.path = $2 AND d.active = TRUE
            "#,
            &[&collection, &path],
        )
        .map_err(pg_error)?;

    Ok(row.map(|r| StoredDocument {
        collection: r.get(0),
        path: r.get(1),
        title: r.get(2),
        body: r.get(3),
        default_from_line: None,
    }))
}

fn get_document_by_suffix(conn: &mut Client, lookup: &str) -> QmdResult<Option<StoredDocument>> {
    let suffix = format!("%{lookup}");
    let row = conn
        .query_opt(
            r#"
            SELECT d.collection, d.path, d.title, c.doc
            FROM documents d
            JOIN content c ON c.hash = d.hash
            WHERE d.active = TRUE
            AND (d.collection || '/' || d.path) ILIKE $1
            ORDER BY LENGTH(d.path) ASC
            LIMIT 1
            "#,
            &[&suffix],
        )
        .map_err(pg_error)?;

    Ok(row.map(|r| StoredDocument {
        collection: r.get(0),
        path: r.get(1),
        title: r.get(2),
        body: r.get(3),
        default_from_line: None,
    }))
}

#[derive(Debug)]
struct FtsRow {
    id: i64,
    hash: String,
    collection: String,
    path: String,
    title: String,
    body: String,
    rank: f64,
}

#[derive(Debug)]
struct VectorRow {
    id: i64,
    hash: String,
    collection: String,
    path: String,
    title: String,
    body: String,
    qvec_blob: Vec<u8>,
}

fn run_fts_query(
    conn: &mut Client,
    fts_query: &str,
    limit: usize,
    collection: Option<&str>,
) -> QmdResult<Vec<FtsRow>> {
    let rows = if let Some(collection) = collection {
        conn.query(
            r#"
            SELECT d.id, d.hash, d.collection, d.path, d.title, c.doc,
                   ts_rank_cd(d.search, websearch_to_tsquery('simple', $1))::float8 AS rank
            FROM documents d
            JOIN content c ON c.hash = d.hash
            WHERE d.active = TRUE
              AND d.collection = $3
              AND d.search @@ websearch_to_tsquery('simple', $1)
            ORDER BY rank DESC
            LIMIT $2
            "#,
            &[&fts_query, &(limit as i64), &collection],
        )
        .map_err(pg_error)?
    } else {
        conn.query(
            r#"
            SELECT d.id, d.hash, d.collection, d.path, d.title, c.doc,
                   ts_rank_cd(d.search, websearch_to_tsquery('simple', $1))::float8 AS rank
            FROM documents d
            JOIN content c ON c.hash = d.hash
            WHERE d.active = TRUE
              AND d.search @@ websearch_to_tsquery('simple', $1)
            ORDER BY rank DESC
            LIMIT $2
            "#,
            &[&fts_query, &(limit as i64)],
        )
        .map_err(pg_error)?
    };

    Ok(rows
        .into_iter()
        .map(|row| FtsRow {
            id: row.get(0),
            hash: row.get(1),
            collection: row.get(2),
            path: row.get(3),
            title: row.get(4),
            body: row.get(5),
            rank: row.get::<_, Option<f64>>(6).unwrap_or(0.0),
        })
        .collect())
}

fn run_path_title_fallback(
    conn: &mut Client,
    query: &str,
    limit: usize,
    collection: Option<&str>,
) -> QmdResult<Vec<FtsRow>> {
    let q = format!("%{}%", query.trim());
    let rows = if let Some(collection) = collection {
        conn.query(
            r#"
            SELECT d.id, d.hash, d.collection, d.path, d.title, c.doc, 0.10::float8 AS rank
            FROM documents d
            JOIN content c ON c.hash = d.hash
            WHERE d.active = TRUE
              AND d.collection = $3
              AND (d.path ILIKE $1 OR d.title ILIKE $1)
            LIMIT $2
            "#,
            &[&q, &(limit as i64), &collection],
        )
        .map_err(pg_error)?
    } else {
        conn.query(
            r#"
            SELECT d.id, d.hash, d.collection, d.path, d.title, c.doc, 0.10::float8 AS rank
            FROM documents d
            JOIN content c ON c.hash = d.hash
            WHERE d.active = TRUE
              AND (d.path ILIKE $1 OR d.title ILIKE $1)
            LIMIT $2
            "#,
            &[&q, &(limit as i64)],
        )
        .map_err(pg_error)?
    };

    Ok(rows
        .into_iter()
        .map(|row| FtsRow {
            id: row.get(0),
            hash: row.get(1),
            collection: row.get(2),
            path: row.get(3),
            title: row.get(4),
            body: row.get(5),
            rank: row.get::<_, Option<f64>>(6).unwrap_or(0.1),
        })
        .collect())
}

fn run_vector_prefilter(
    conn: &mut Client,
    lsh_buckets: &[(i64, i64)],
    collection: Option<&str>,
) -> QmdResult<Vec<VectorRow>> {
    if lsh_buckets.is_empty() {
        return Ok(Vec::new());
    }

    let values_sql = lsh_buckets
        .iter()
        .map(|(band, bucket)| format!("({}, {})", *band as i32, bucket))
        .collect::<Vec<_>>()
        .join(", ");

    let mut sql = format!(
        r#"
        WITH target_buckets(band, bucket) AS (VALUES {values_sql})
        SELECT DISTINCT d.id, d.hash, d.collection, d.path, d.title, c.doc, v.qvec
        FROM documents d
        JOIN content c ON c.hash = d.hash
        JOIN content_vectors_native v ON v.hash = d.hash
        WHERE d.active = TRUE
          AND EXISTS (
              SELECT 1
              FROM content_vectors_lsh l
              JOIN target_buckets t
                ON t.band = l.band AND t.bucket = l.bucket
              WHERE l.hash = d.hash
          )
        "#
    );

    if collection.is_some() {
        sql.push_str(" AND d.collection = $1");
    }
    sql.push_str(" LIMIT 4000");

    let rows = if let Some(collection) = collection {
        conn.query(&sql, &[&collection]).map_err(pg_error)?
    } else {
        conn.query(&sql, &[]).map_err(pg_error)?
    };

    Ok(rows
        .into_iter()
        .map(|row| VectorRow {
            id: row.get(0),
            hash: row.get(1),
            collection: row.get(2),
            path: row.get(3),
            title: row.get(4),
            body: row.get(5),
            qvec_blob: row.get(6),
        })
        .collect())
}

fn run_vector_scan_all(conn: &mut Client, collection: Option<&str>) -> QmdResult<Vec<VectorRow>> {
    let rows = if let Some(collection) = collection {
        conn.query(
            r#"
            SELECT d.id, d.hash, d.collection, d.path, d.title, c.doc, v.qvec
            FROM documents d
            JOIN content c ON c.hash = d.hash
            JOIN content_vectors_native v ON v.hash = d.hash
            WHERE d.active = TRUE AND d.collection = $1
            "#,
            &[&collection],
        )
        .map_err(pg_error)?
    } else {
        conn.query(
            r#"
            SELECT d.id, d.hash, d.collection, d.path, d.title, c.doc, v.qvec
            FROM documents d
            JOIN content c ON c.hash = d.hash
            JOIN content_vectors_native v ON v.hash = d.hash
            WHERE d.active = TRUE
            "#,
            &[],
        )
        .map_err(pg_error)?
    };

    Ok(rows
        .into_iter()
        .map(|row| VectorRow {
            id: row.get(0),
            hash: row.get(1),
            collection: row.get(2),
            path: row.get(3),
            title: row.get(4),
            body: row.get(5),
            qvec_blob: row.get(6),
        })
        .collect())
}

fn parse_file_lookup(file: &str) -> (&str, Option<usize>) {
    if let Some((head, tail)) = file.rsplit_once(':') {
        if let Ok(line) = tail.parse::<usize>() {
            return (head, Some(line));
        }
    }
    (file, None)
}

fn split_collection_path(input: &str) -> Option<(&str, &str)> {
    let mut parts = input.splitn(2, '/');
    let collection = parts.next()?;
    let path = parts.next()?;
    if collection.is_empty() || path.is_empty() {
        return None;
    }
    Some((collection, path))
}

fn path_quality_multiplier(file: &str) -> f32 {
    let lowered = file.to_ascii_lowercase();
    let mut mult = 1.0_f32;

    if lowered.contains("/crates/") {
        mult *= 1.08;
    }
    if lowered.contains("/src/") {
        mult *= 1.05;
    }

    for noisy in [
        "/target/",
        "/dist/",
        "/build/",
        "/node_modules/",
        "/deepwiki-rs/",
        "/generated/",
        "/.git/",
    ] {
        if lowered.contains(noisy) {
            mult *= 0.72;
        }
    }

    mult.clamp(0.50, 1.20)
}

fn emit_search_telemetry(
    mode: &str,
    query: &str,
    limit: usize,
    result_count: usize,
    duration: Duration,
) {
    let path = search_telemetry_path();
    let Some(path) = path else {
        return;
    };

    let payload = serde_json::json!({
        "ts": Utc::now().to_rfc3339(),
        "mode": mode,
        "query": query,
        "limit": limit,
        "result_count": result_count,
        "duration_ms": duration.as_millis(),
    });

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{payload}");
    }
}

fn search_telemetry_path() -> Option<PathBuf> {
    let cfg = repo_qmd_config();
    cfg.paths
        .search_telemetry_path
        .or_else(|| std::env::var("QMD_SEARCH_TELEMETRY_PATH").ok())
        .map(PathBuf::from)
}

fn normalize_context_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return "/".to_string();
    }
    let with_prefix = if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    };
    with_prefix.trim_end_matches('/').to_string()
}

fn extract_snippet(body: &str, query: &str, max_chars: usize) -> String {
    if body.is_empty() {
        return String::new();
    }

    if let Some(idx) = body.to_lowercase().find(&query.to_lowercase()) {
        let start = idx.saturating_sub(max_chars / 4);
        let end = (idx + (max_chars * 3 / 4)).min(body.len());
        return body[start..end].trim().to_string();
    }

    body.chars().take(max_chars).collect()
}

fn extract_snippet_multi(body: &str, terms: &[String], max_chars: usize) -> String {
    if body.is_empty() {
        return String::new();
    }
    for term in terms {
        if term.is_empty() {
            continue;
        }
        if let Some(idx) = body.to_lowercase().find(term) {
            let start = idx.saturating_sub(max_chars / 4);
            let end = (idx + (max_chars * 3 / 4)).min(body.len());
            return body[start..end].trim().to_string();
        }
    }
    body.chars().take(max_chars).collect()
}

fn tokenize(input: &str) -> Vec<String> {
    input
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|s| s.len() >= 2)
        .map(str::to_string)
        .collect()
}

fn build_query_variants(query: &str) -> Vec<(f32, String)> {
    let cleaned = query.trim();
    if cleaned.is_empty() {
        return vec![];
    }
    let terms = tokenize(cleaned);
    let mut out = Vec::new();
    out.push((1.0, cleaned.to_string()));
    if !terms.is_empty() {
        out.push((1.08, terms.join(" ")));
        if terms.len() > 1 {
            out.push((0.92, format!("{} {}", terms[0], terms[terms.len() - 1])));
        }
    }
    out
}

fn term_overlap_ratio(query_terms: &[String], title: &str, body: &str) -> f32 {
    if query_terms.is_empty() {
        return 0.0;
    }
    let hay = format!("{} {}", title.to_lowercase(), body.to_lowercase());
    let matched = query_terms
        .iter()
        .filter(|term| hay.contains(term.as_str()))
        .count();
    matched as f32 / query_terms.len() as f32
}

fn derive_title(path: &str, body: &str) -> String {
    for line in body.lines().take(20) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(title) = trimmed.strip_prefix("# ") {
            return title.trim().to_string();
        }
        if trimmed.len() >= 5 {
            return trimmed.chars().take(120).collect();
        }
    }
    Path::new(path)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(path)
        .to_string()
}

fn filesystem_modified_utc(path: &Path) -> DateTime<Utc> {
    match fs::metadata(path).and_then(|m| m.modified()) {
        Ok(modified) => DateTime::<Utc>::from(modified),
        Err(_) => Utc::now(),
    }
}

fn embed_text_native(text: &str, dim: usize) -> Vec<f32> {
    let mut vec = vec![0.0_f32; dim];
    for token in tokenize(text) {
        let h = blake3::hash(token.as_bytes());
        let bytes = h.as_bytes();
        let idx = (((bytes[0] as usize) << 8) | bytes[1] as usize) % dim;
        let sign = if bytes[2] & 1 == 0 { 1.0 } else { -1.0 };
        let tf = 1.0 + (token.len() as f32).ln();
        vec[idx] += sign * tf;
    }
    l2_normalize(&mut vec);
    vec
}

fn l2_normalize(vec: &mut [f32]) {
    let norm = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 1e-6 {
        for value in vec {
            *value /= norm;
        }
    }
}

fn quantize_embedding(vec: &[f32]) -> Vec<i16> {
    vec.iter()
        .map(|v| (v.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
        .collect()
}

fn encode_qvec(vec: &[i16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(vec.len() * 2);
    for value in vec {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

fn decode_qvec(blob: &[u8]) -> QmdResult<Vec<i16>> {
    if blob.len() % 2 != 0 {
        return Err(QmdError::Internal(
            "native vector blob has invalid byte length".to_string(),
        ));
    }
    let mut out = Vec::with_capacity(blob.len() / 2);
    let mut bytes = blob;
    while !bytes.is_empty() {
        let (head, tail) = bytes.split_at(2);
        out.push(i16::from_le_bytes([head[0], head[1]]));
        bytes = tail;
    }
    Ok(out)
}

fn cosine_similarity_qvec(a: &[i16], b: &[i16]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0_f32;
    let mut na = 0_f32;
    let mut nb = 0_f32;
    for (av, bv) in a.iter().zip(b.iter()) {
        let af = *av as f32 / i16::MAX as f32;
        let bf = *bv as f32 / i16::MAX as f32;
        dot += af * bf;
        na += af * af;
        nb += bf * bf;
    }
    if na <= 1e-6 || nb <= 1e-6 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

fn compute_lsh_buckets(qvec: &[i16], bands: usize) -> Vec<(i64, i64)> {
    if qvec.is_empty() || bands == 0 {
        return Vec::new();
    }
    let segment = (qvec.len() / bands).max(1);
    let mut out = Vec::new();
    for band in 0..bands {
        let start = band * segment;
        if start >= qvec.len() {
            break;
        }
        let end = ((band + 1) * segment).min(qvec.len());
        let mut bytes = Vec::with_capacity((end - start) * 2);
        for value in &qvec[start..end] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let digest = blake3::hash(&bytes);
        let hash = u64::from_le_bytes([
            digest.as_bytes()[0],
            digest.as_bytes()[1],
            digest.as_bytes()[2],
            digest.as_bytes()[3],
            digest.as_bytes()[4],
            digest.as_bytes()[5],
            digest.as_bytes()[6],
            digest.as_bytes()[7],
        ]);
        let bucket = (hash % 1_000_003) as i64;
        out.push((band as i64, bucket));
    }
    out
}

fn slice_text(
    body: &str,
    from_line: Option<usize>,
    max_lines: Option<usize>,
    line_numbers: bool,
) -> String {
    let lines = body.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return String::new();
    }

    let start = from_line.unwrap_or(1).saturating_sub(1);
    if start >= lines.len() {
        return String::new();
    }

    let end = max_lines
        .map(|n| start.saturating_add(n).min(lines.len()))
        .unwrap_or(lines.len());

    let mut out = Vec::with_capacity(end - start);
    for (index, line) in lines[start..end].iter().enumerate() {
        if line_numbers {
            out.push(format!("{}: {}", start + index + 1, line));
        } else {
            out.push((*line).to_string());
        }
    }
    out.join("\n")
}

fn parse_postgres_config(database_url: &str) -> QmdResult<r2d2_postgres::postgres::Config> {
    if database_url.starts_with("postgres://") || database_url.starts_with("postgresql://") {
        let parsed = url::Url::parse(database_url)
            .map_err(|e| QmdError::Internal(format!("invalid postgres url: {e}")))?;

        let mut cfg = r2d2_postgres::postgres::Config::new();
        if let Some(host) = parsed.host_str() {
            cfg.host(host);
        }
        if let Some(port) = parsed.port() {
            cfg.port(port);
        }

        if !parsed.username().is_empty() {
            cfg.user(parsed.username());
        }
        if let Some(password) = parsed.password() {
            cfg.password(password);
        }

        let dbname = parsed.path().trim_start_matches('/');
        if !dbname.is_empty() {
            cfg.dbname(dbname);
        }

        for (key, value) in parsed.query_pairs() {
            if key.eq_ignore_ascii_case("sslmode")
                && let Some(mode) = parse_ssl_mode(value.as_ref())
            {
                cfg.ssl_mode(mode);
            }
        }
        return Ok(cfg);
    }

    let cfg = database_url
        .parse::<r2d2_postgres::postgres::Config>()
        .map_err(|e| QmdError::Internal(format!("invalid postgres config string: {e}")))?;
    Ok(cfg)
}

fn dsn_has_password(database_url: &str) -> bool {
    if database_url.contains("password=") {
        return true;
    }
    if let Some(authority) = database_url
        .strip_prefix("postgresql://")
        .or_else(|| database_url.strip_prefix("postgres://"))
        .and_then(|rest| rest.split('/').next())
    {
        return authority
            .split('@')
            .next()
            .map(|userinfo| userinfo.contains(':'))
            .unwrap_or(false);
    }
    false
}

fn missing_password_hint(database_url: &str) -> &'static str {
    if dsn_has_password(database_url) || runtime_password().is_some() {
        ""
    } else {
        "; hint: this PostgreSQL instance may require SCRAM password auth; set QMD_DATABASE_URL with password or set PGPASSWORD"
    }
}

fn parse_ssl_mode(input: &str) -> Option<SslMode> {
    match input.to_ascii_lowercase().as_str() {
        "disable" => Some(SslMode::Disable),
        "prefer" => Some(SslMode::Prefer),
        "require" => Some(SslMode::Require),
        _ => None,
    }
}

fn sanitize_index_name(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

fn default_config_path(index_name: &str) -> QmdResult<PathBuf> {
    if let Ok(dir) = std::env::var("QMD_CONFIG_DIR") {
        let mut path = PathBuf::from(dir);
        path.push(format!("{index_name}.yml"));
        return Ok(path);
    }

    let mut base = dirs::config_dir().ok_or_else(|| {
        QmdError::Internal("unable to determine user config directory".to_string())
    })?;
    base.push("qmd");
    base.push(format!("{index_name}.yml"));
    Ok(base)
}

fn resolve_collection_config_path(index_name: &str) -> QmdResult<PathBuf> {
    if let Some(path) = repo_qmd_config().paths.collections_config_path
        && !path.trim().is_empty()
    {
        return Ok(PathBuf::from(path));
    }
    default_config_path(index_name)
}

fn resolve_runtime_db_settings(index_name: &str) -> RuntimeDbSettings {
    let fallback_url = build_default_database_url(index_name);
    default_runtime_db_settings(fallback_url)
}

fn default_runtime_db_settings(fallback_url: String) -> RuntimeDbSettings {
    let cfg = repo_qmd_config();
    let dot = repo_dotenv_values();
    let db = cfg.database;
    let sslmode = db
        .sslmode
        .clone()
        .or_else(|| get_dotenv_or_env(&dot, "QMD_DB_SSLMODE"))
        .or_else(|| get_dotenv_or_env(&dot, "QMD_PG_SSLMODE"))
        .unwrap_or_else(|| "disable".to_string());

    let database_url = db
        .url
        .or_else(|| get_dotenv_or_env(&dot, "QMD_DATABASE_URL"))
        .or_else(|| get_dotenv_or_env(&dot, "DATABASE_URL"))
        .unwrap_or_else(|| {
            let host = db
                .host
                .clone()
                .or_else(|| get_dotenv_or_env(&dot, "QMD_DB_HOST"))
                .or_else(|| get_dotenv_or_env(&dot, "PGHOST"))
                .unwrap_or_else(|| "127.0.0.1".to_string());
            let port = db
                .port
                .or_else(|| get_dotenv_or_env(&dot, "QMD_DB_PORT").and_then(|v| v.parse().ok()))
                .or_else(|| get_dotenv_or_env(&dot, "PGPORT").and_then(|v| v.parse().ok()))
                .unwrap_or(5432);
            let user = db
                .user
                .clone()
                .or_else(|| get_dotenv_or_env(&dot, "QMD_DB_USER"))
                .or_else(|| get_dotenv_or_env(&dot, "PGUSER"))
                .unwrap_or_else(|| "postgres".to_string());
            let pass = db
                .password
                .clone()
                .or_else(|| get_dotenv_or_env(&dot, "QMD_DB_PASSWORD"))
                .or_else(|| get_dotenv_or_env(&dot, "PGPASSWORD"))
                .unwrap_or_else(|| "password".to_string());
            let name = db
                .name
                .clone()
                .or_else(|| get_dotenv_or_env(&dot, "QMD_DB_NAME"))
                .or_else(|| get_dotenv_or_env(&dot, "PGDATABASE"))
                .unwrap_or_else(|| "qmd_index".to_string());
            format!("postgresql://{user}:{pass}@{host}:{port}/{name}?sslmode={sslmode}")
        });

    let pool_max = db
        .pool_max
        .or_else(|| get_dotenv_or_env(&dot, "QMD_PG_POOL_MAX").and_then(|v| v.parse().ok()))
        .unwrap_or(24)
        .max(4);
    let pool_min_idle = db
        .pool_min_idle
        .or_else(|| get_dotenv_or_env(&dot, "QMD_PG_POOL_MIN_IDLE").and_then(|v| v.parse().ok()))
        .unwrap_or(4)
        .min(pool_max);
    let pool_connect_timeout_ms = db
        .pool_connect_timeout_ms
        .or_else(|| {
            get_dotenv_or_env(&dot, "QMD_PG_POOL_CONNECT_TIMEOUT_MS").and_then(|v| v.parse().ok())
        })
        .unwrap_or(8_000);
    let pool_idle_timeout_ms = db
        .pool_idle_timeout_ms
        .or_else(|| {
            get_dotenv_or_env(&dot, "QMD_PG_POOL_IDLE_TIMEOUT_MS").and_then(|v| v.parse().ok())
        })
        .unwrap_or(600_000);
    let pool_max_lifetime_ms = db
        .pool_max_lifetime_ms
        .or_else(|| {
            get_dotenv_or_env(&dot, "QMD_PG_POOL_MAX_LIFETIME_MS").and_then(|v| v.parse().ok())
        })
        .unwrap_or(3_600_000);
    let pool_test_on_check_out = db
        .pool_test_on_check_out
        .or_else(|| {
            parse_bool_opt(get_dotenv_or_env(&dot, "QMD_PG_POOL_TEST_ON_CHECKOUT").as_deref())
        })
        .unwrap_or(true);
    let db_connect_timeout_ms = db
        .db_connect_timeout_ms
        .or_else(|| {
            get_dotenv_or_env(&dot, "QMD_PG_DB_CONNECT_TIMEOUT_MS").and_then(|v| v.parse().ok())
        })
        .unwrap_or(5_000);

    let allow_tls = db
        .allow_tls
        .or_else(|| parse_bool_opt(get_dotenv_or_env(&dot, "QMD_PG_ALLOW_TLS").as_deref()))
        .unwrap_or(true);
    let allow_insecure_tls = db
        .allow_insecure_tls
        .or_else(|| parse_bool_opt(get_dotenv_or_env(&dot, "QMD_PG_TLS_INSECURE").as_deref()))
        .unwrap_or(false);
    let bootstrap = db
        .bootstrap
        .or_else(|| parse_bool_opt(get_dotenv_or_env(&dot, "QMD_PG_BOOTSTRAP").as_deref()))
        .unwrap_or(true);
    let admin_db = db
        .admin_db
        .or_else(|| get_dotenv_or_env(&dot, "QMD_PG_ADMIN_DB"))
        .unwrap_or_else(|| "postgres".to_string());

    RuntimeDbSettings {
        database_url: if database_url.is_empty() {
            fallback_url
        } else {
            database_url
        },
        admin_db,
        bootstrap,
        pool_max,
        pool_min_idle,
        pool_connect_timeout_ms,
        pool_idle_timeout_ms,
        pool_max_lifetime_ms,
        pool_test_on_check_out,
        db_connect_timeout_ms,
        allow_tls,
        allow_insecure_tls,
    }
}

fn build_default_database_url(index_name: &str) -> String {
    let host = "127.0.0.1";
    let port = 5432;
    let user = "postgres";
    let pass = "password";
    let db = format!("qmd_{}", sanitize_index_name(index_name));
    format!("postgresql://{user}:{pass}@{host}:{port}/{db}")
}

fn runtime_password() -> Option<String> {
    let dot = repo_dotenv_values();
    let db = repo_qmd_config().database;
    db.password
        .or_else(|| get_dotenv_or_env(&dot, "QMD_DB_PASSWORD"))
        .or_else(|| get_dotenv_or_env(&dot, "PGPASSWORD"))
}

fn repo_qmd_config() -> RepoQmdConfig {
    let config_path = discover_repo_file("qmd.config.json")
        .or_else(|| discover_repo_file("config/qmd.config.json"));
    let Some(path) = config_path else {
        return RepoQmdConfig::default();
    };

    let raw = match fs::read_to_string(path) {
        Ok(v) => v,
        Err(_) => return RepoQmdConfig::default(),
    };
    serde_json::from_str::<RepoQmdConfig>(&raw).unwrap_or_default()
}

fn repo_dotenv_values() -> BTreeMap<String, String> {
    let env_path = discover_repo_file(".env").or_else(|| discover_repo_file("config/.env"));
    let Some(path) = env_path else {
        return BTreeMap::new();
    };
    parse_env_file(&path).unwrap_or_default()
}

fn parse_env_file(path: &Path) -> QmdResult<BTreeMap<String, String>> {
    let content = fs::read_to_string(path).map_err(io_error)?;
    let mut out = BTreeMap::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((k, v)) = trimmed.split_once('=') else {
            continue;
        };
        let key = k.trim().to_string();
        let mut value = v.trim().to_string();
        if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
            value = value[1..value.len() - 1].to_string();
        }
        out.insert(key, value);
    }
    Ok(out)
}

fn discover_repo_file(relative: &str) -> Option<PathBuf> {
    let mut cursor = std::env::current_dir().ok()?;
    loop {
        let candidate = cursor.join(relative);
        if candidate.exists() {
            return Some(candidate);
        }
        if !cursor.pop() {
            break;
        }
    }

    let known = PathBuf::from("C:/codedev/litho-workspace").join(relative);
    if known.exists() {
        return Some(known);
    }
    None
}

fn get_dotenv_or_env(dotenv: &BTreeMap<String, String>, key: &str) -> Option<String> {
    dotenv.get(key).cloned().or_else(|| std::env::var(key).ok())
}

fn parse_bool_opt(input: Option<&str>) -> Option<bool> {
    match input?.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn bootstrap_database_if_needed(
    config: &r2d2_postgres::postgres::Config,
    admin_db: &str,
    allow_tls: bool,
    allow_insecure_tls: bool,
) -> QmdResult<()> {
    let target_db = config.get_dbname().unwrap_or("postgres").to_string();
    if target_db.eq_ignore_ascii_case(admin_db) {
        return Ok(());
    }

    let mut probe = config.clone();
    probe.ssl_mode(SslMode::Disable);
    match probe.connect(NoTls) {
        Ok(_) => return Ok(()),
        Err(err) if !is_missing_database_error(&err) => return Ok(()),
        Err(_) => {}
    }

    let mut admin_cfg = config.clone();
    admin_cfg.dbname(admin_db);
    admin_cfg.ssl_mode(SslMode::Disable);
    let mut admin = match admin_cfg.connect(NoTls) {
        Ok(client) => client,
        Err(no_tls_err) => {
            if !allow_tls {
                return Err(QmdError::Internal(format!(
                    "postgres bootstrap failed: cannot connect to admin db '{admin_db}' without TLS: {no_tls_err}"
                )));
            }
            let mut tls_builder = TlsConnector::builder();
            if allow_insecure_tls {
                tls_builder.danger_accept_invalid_certs(true);
            }
            let tls_connector = tls_builder
                .build()
                .map_err(|e| QmdError::Internal(format!("failed to build TLS connector: {e}")))?;
            admin_cfg
                .connect(MakeTlsConnector::new(tls_connector))
                .map_err(|e| {
                    QmdError::Internal(format!(
                        "postgres bootstrap failed: admin db TLS connection error: {e}"
                    ))
                })?
        }
    };

    let exists: bool = admin
        .query_one(
            "SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname = $1)",
            &[&target_db],
        )
        .map_err(pg_error)?
        .get(0);
    if exists {
        return Ok(());
    }

    let safe_db = validate_pg_identifier(&target_db)?;
    let owner = config
        .get_user()
        .map(validate_pg_identifier)
        .transpose()?
        .unwrap_or("postgres");
    admin
        .batch_execute(&format!(
            "CREATE DATABASE {} OWNER {}",
            quote_pg_identifier(safe_db),
            quote_pg_identifier(owner)
        ))
        .map_err(pg_error)?;

    Ok(())
}

fn is_missing_database_error(err: &r2d2_postgres::postgres::Error) -> bool {
    err.as_db_error()
        .map(|e| e.code() == &r2d2_postgres::postgres::error::SqlState::INVALID_CATALOG_NAME)
        .unwrap_or(false)
}

fn validate_pg_identifier(input: &str) -> QmdResult<&str> {
    let valid = !input.is_empty()
        && input
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if valid {
        Ok(input)
    } else {
        Err(QmdError::InvalidRequest(format!(
            "invalid postgres identifier: {input}"
        )))
    }
}

fn quote_pg_identifier(input: &str) -> String {
    format!("\"{}\"", input.replace('"', "\"\""))
}

fn default_pattern() -> String {
    DEFAULT_PATTERN.to_string()
}

fn pool_error(err: r2d2::Error) -> QmdError {
    QmdError::Internal(format!("postgres pool error: {err} ({err:?})"))
}

fn pg_error(err: r2d2_postgres::postgres::Error) -> QmdError {
    QmdError::Internal(format!("postgres error: {err} ({err:?})"))
}

fn io_error(err: std::io::Error) -> QmdError {
    QmdError::Internal(format!("io error: {err}"))
}
