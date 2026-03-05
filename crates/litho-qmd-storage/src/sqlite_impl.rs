use super::*;

pub(super) struct SqliteRuntimeSettings {
    pub(super) database_path: PathBuf,
}

pub(super) fn resolve_backend_kind(index_name: &str) -> QmdBackendKind {
    let cfg = repo_qmd_config();
    let dot = repo_dotenv_values();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let git_root = discover_git_root(&cwd);
    resolve_backend_kind_for(index_name, &cfg, &dot, &cwd, git_root.as_deref())
}

fn resolve_backend_kind_for(
    index_name: &str,
    cfg: &RepoQmdConfig,
    dot: &BTreeMap<String, String>,
    cwd: &Path,
    git_root: Option<&Path>,
) -> QmdBackendKind {
    if let Some(explicit) = explicit_backend(cfg, dot) {
        return explicit;
    }

    if cfg.database.sqlite_path.as_deref().is_some_and(non_empty)
        || get_dotenv_or_env(dot, "QMD_SQLITE_PATH")
            .as_deref()
            .is_some_and(non_empty)
    {
        return QmdBackendKind::Sqlite;
    }

    if find_existing_sqlite_path(index_name, cwd, git_root).is_some() {
        return QmdBackendKind::Sqlite;
    }

    if git_root.is_some() {
        return QmdBackendKind::Sqlite;
    }

    if has_explicit_postgres_config(cfg, dot) {
        return QmdBackendKind::Postgres;
    }

    QmdBackendKind::Postgres
}

pub(super) fn resolve_sqlite_runtime(index_name: &str) -> SqliteRuntimeSettings {
    let cfg = repo_qmd_config();
    let dot = repo_dotenv_values();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let git_root = discover_git_root(&cwd);

    let database_path = get_dotenv_or_env(&dot, "QMD_SQLITE_PATH")
        .filter(|v| non_empty(v))
        .map(PathBuf::from)
        .or_else(|| {
            cfg.database
                .sqlite_path
                .as_ref()
                .filter(|v| non_empty(v))
                .map(PathBuf::from)
        })
        .or_else(|| find_existing_sqlite_path(index_name, &cwd, git_root.as_deref()))
        .unwrap_or_else(|| default_sqlite_path(index_name, &cwd, git_root.as_deref()));

    SqliteRuntimeSettings { database_path }
}

impl QmdStore for SqliteQmdStore {
    fn status(&self) -> QmdResult<IndexStatus> {
        let config = self.load_config()?;
        let (total_documents, needs_embedding, has_vector_index, counts, updates) =
            self.with_conn(|conn| {
                let total_documents = conn
                    .query_row(
                        "SELECT COUNT(*) FROM documents WHERE active = 1",
                        [],
                        |row| row.get::<_, i64>(0),
                    )
                    .map_err(sqlite_error)? as usize;

                let needs_embedding = conn
                    .query_row(
                        r#"
                        SELECT COUNT(*)
                        FROM documents d
                        WHERE d.active = 1
                          AND NOT EXISTS (
                            SELECT 1 FROM content_vectors_native cv WHERE cv.hash = d.hash
                          )
                        "#,
                        [],
                        |row| row.get::<_, i64>(0),
                    )
                    .map_err(sqlite_error)? as usize;

                let has_vector_index = conn
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'content_vectors_native')",
                        [],
                        |row| row.get::<_, i64>(0),
                    )
                    .map_err(sqlite_error)?
                    != 0;

                let mut counts = BTreeMap::new();
                let mut count_stmt = conn
                    .prepare(
                        "SELECT collection, COUNT(*) FROM documents WHERE active = 1 GROUP BY collection",
                    )
                    .map_err(sqlite_error)?;
                let count_rows = count_stmt
                    .query_map([], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                    })
                    .map_err(sqlite_error)?;
                for row in count_rows {
                    let (name, count) = row.map_err(sqlite_error)?;
                    counts.insert(name, count as usize);
                }

                let mut updates = BTreeMap::new();
                let mut update_stmt = conn
                    .prepare(
                        "SELECT collection, MAX(modified_at) FROM documents WHERE active = 1 GROUP BY collection",
                    )
                    .map_err(sqlite_error)?;
                let update_rows = update_stmt
                    .query_map([], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
                    })
                    .map_err(sqlite_error)?;
                for row in update_rows {
                    let (name, modified) = row.map_err(sqlite_error)?;
                    updates.insert(name, modified.unwrap_or_default());
                }

                Ok((
                    total_documents,
                    needs_embedding,
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
            let rows = run_sqlite_lexical_scan(conn, options.collection.as_deref())?;
            let strategies = build_query_variants(query);
            let mut fused = BTreeMap::<String, SearchHit>::new();

            for (strategy_weight, strategy_query) in strategies {
                let strategy_terms = tokenize(&strategy_query);
                let strategy_lower = strategy_query.to_ascii_lowercase();
                for row in &rows {
                    let file = format!("{}/{}", row.collection, row.path);
                    let hay = format!(
                        "{} {} {}",
                        row.path.to_ascii_lowercase(),
                        row.title.to_ascii_lowercase(),
                        row.body.to_ascii_lowercase()
                    );
                    let contains = if hay.contains(&strategy_lower) {
                        1.0
                    } else {
                        0.0
                    };
                    let overlap = term_overlap_ratio(&strategy_terms, &row.title, &row.body);
                    if overlap <= 0.0 && contains <= 0.0 {
                        continue;
                    }
                    let score = (0.78 * overlap + 0.22 * contains)
                        * strategy_weight
                        * path_quality_multiplier(&file);
                    let context = SqliteQmdStore::context_for_path(&config, &file);
                    let hit = SearchHit {
                        docid: format!("#{}", row.id),
                        file: file.clone(),
                        title: row.title.clone(),
                        score,
                        context,
                        snippet: extract_snippet_multi(&row.body, &strategy_terms, 360),
                    };
                    match fused.get(&row.hash) {
                        Some(existing) if existing.score >= hit.score => {}
                        _ => {
                            fused.insert(row.hash.clone(), hit);
                        }
                    }
                }
            }

            if fused.is_empty() {
                let q = query.to_ascii_lowercase();
                for row in rows {
                    let file = format!("{}/{}", row.collection, row.path);
                    if !row.path.to_ascii_lowercase().contains(&q)
                        && !row.title.to_ascii_lowercase().contains(&q)
                    {
                        continue;
                    }
                    let context = SqliteQmdStore::context_for_path(&config, &file);
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
            let table_exists = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'content_vectors_native')",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(sqlite_error)?
                != 0;
            if !table_exists {
                return Ok(Vec::new());
            }

            let rows = run_sqlite_vector_scan_all(conn, options.collection.as_deref())?;
            let query_terms = tokenize(query);
            let mut scored = BTreeMap::<String, SearchHit>::new();
            for row in rows {
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
                let context = SqliteQmdStore::context_for_path(&config, &file);
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
        let context = SqliteQmdStore::context_for_path(&config, &display_path);

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
            let mut out = Vec::new();
            match prefix {
                Some(prefix) => {
                    let like = format!("{}%", prefix.to_ascii_lowercase());
                    let mut stmt = conn
                        .prepare(
                            r#"
                            SELECT collection || '/' || path AS display_path
                            FROM documents
                            WHERE active = 1
                              AND lower(collection || '/' || path) LIKE ?1
                            ORDER BY display_path
                            "#,
                        )
                        .map_err(sqlite_error)?;
                    let rows = stmt
                        .query_map(params![like], |row| row.get::<_, String>(0))
                        .map_err(sqlite_error)?;
                    for row in rows {
                        out.push(row.map_err(sqlite_error)?);
                    }
                }
                None => {
                    let mut stmt = conn
                        .prepare(
                            r#"
                            SELECT collection || '/' || path AS display_path
                            FROM documents
                            WHERE active = 1
                            ORDER BY display_path
                            "#,
                        )
                        .map_err(sqlite_error)?;
                    let rows = stmt
                        .query_map([], |row| row.get::<_, String>(0))
                        .map_err(sqlite_error)?;
                    for row in rows {
                        out.push(row.map_err(sqlite_error)?);
                    }
                }
            }
            Ok(out)
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
                modified_at: String,
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
                let modified_at = filesystem_modified_utc(path).to_rfc3339();
                candidates.push(Candidate {
                    rel_norm,
                    hash,
                    title,
                    modified_at,
                    content,
                });
            }

            self.with_conn(|conn| {
                let tx = conn.transaction().map_err(sqlite_error)?;

                let mut existing_map = FastHashTable::<String, String>::with_capacity(
                    candidates.len().saturating_mul(2).max(64),
                );
                {
                    let mut stmt = tx
                        .prepare("SELECT path, hash FROM documents WHERE collection = ?1")
                        .map_err(sqlite_error)?;
                    let rows = stmt
                        .query_map(params![collection_name], |row| {
                            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                        })
                        .map_err(sqlite_error)?;
                    for row in rows {
                        let (path, hash) = row.map_err(sqlite_error)?;
                        let _ = existing_map.insert(path, hash);
                    }
                }

                let now = Utc::now().to_rfc3339();
                for candidate in &candidates {
                    tx.execute(
                        "INSERT OR IGNORE INTO content(hash, doc, created_at) VALUES (?1, ?2, ?3)",
                        params![candidate.hash, candidate.content, now],
                    )
                    .map_err(sqlite_error)?;

                    match existing_map.get(&candidate.rel_norm) {
                        Some(existing_hash) if existing_hash == &candidate.hash && !force => {
                            tx.execute(
                                "UPDATE documents SET active = 1, modified_at = ?3 WHERE collection = ?1 AND path = ?2",
                                params![collection_name, candidate.rel_norm, candidate.modified_at],
                            )
                            .map_err(sqlite_error)?;
                            report.unchanged_documents += 1;
                        }
                        Some(_) => {
                            tx.execute(
                                r#"
                                UPDATE documents
                                SET hash = ?3,
                                    title = ?4,
                                    active = 1,
                                    modified_at = ?5,
                                    search = lower(?2 || ' ' || ?4 || ' ' || ?6)
                                WHERE collection = ?1 AND path = ?2
                                "#,
                                params![
                                    collection_name,
                                    candidate.rel_norm,
                                    candidate.hash,
                                    candidate.title,
                                    candidate.modified_at,
                                    candidate.content
                                ],
                            )
                            .map_err(sqlite_error)?;
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
                                VALUES(?1, ?2, ?3, ?4, ?5, ?6, 1, lower(?2 || ' ' || ?3 || ' ' || ?7))
                                "#,
                                params![
                                    collection_name,
                                    candidate.rel_norm,
                                    candidate.title,
                                    candidate.hash,
                                    now,
                                    candidate.modified_at,
                                    candidate.content
                                ],
                            )
                            .map_err(sqlite_error)?;
                            report.indexed_documents += 1;
                        }
                    }
                }

                {
                    let mut stmt = tx
                        .prepare("SELECT path FROM documents WHERE collection = ?1 AND active = 1")
                        .map_err(sqlite_error)?;
                    let rows = stmt
                        .query_map(params![collection_name], |row| row.get::<_, String>(0))
                        .map_err(sqlite_error)?;
                    for row in rows {
                        let old = row.map_err(sqlite_error)?;
                        if !seen_paths.contains(&old) {
                            tx.execute(
                                "UPDATE documents SET active = 0 WHERE collection = ?1 AND path = ?2 AND active = 1",
                                params![collection_name, old],
                            )
                            .map_err(sqlite_error)?;
                            report.deactivated_documents += 1;
                        }
                    }
                }

                tx.commit().map_err(sqlite_error)?;
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
            let mut stmt = conn
                .prepare(
                    r#"
                    SELECT DISTINCT d.hash, c.doc
                    FROM documents d
                    JOIN content c ON c.hash = d.hash
                    WHERE d.active = 1
                    "#,
                )
                .map_err(sqlite_error)?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(sqlite_error)?;
            for row in rows {
                out.push(row.map_err(sqlite_error)?);
            }
            Ok(out)
        })?;

        let existing = self.with_conn(|conn| {
            let mut out = HashSet::new();
            let mut stmt = conn
                .prepare("SELECT hash FROM content_vectors_native")
                .map_err(sqlite_error)?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(sqlite_error)?;
            for row in rows {
                out.insert(row.map_err(sqlite_error)?);
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
            let tx = conn.transaction().map_err(sqlite_error)?;
            for (hash, text) in to_embed {
                let vec = embed_text_native(&text, NATIVE_VECTOR_DIM);
                let qvec = quantize_embedding(&vec);
                let lsh = compute_lsh_buckets(&qvec, 8);

                tx.execute(
                    r#"
                    INSERT INTO content_vectors_native(hash, dim, qvec, model, embedded_at)
                    VALUES(?1, ?2, ?3, ?4, ?5)
                    ON CONFLICT(hash) DO UPDATE SET
                      dim = excluded.dim,
                      qvec = excluded.qvec,
                      model = excluded.model,
                      embedded_at = excluded.embedded_at
                    "#,
                    params![
                        hash,
                        NATIVE_VECTOR_DIM as i32,
                        encode_qvec(&qvec),
                        NATIVE_VECTOR_MODEL,
                        Utc::now().to_rfc3339()
                    ],
                )
                .map_err(sqlite_error)?;

                tx.execute("DELETE FROM content_vectors_lsh WHERE hash = ?1", params![hash])
                    .map_err(sqlite_error)?;
                for (band, bucket) in &lsh {
                    tx.execute(
                        "INSERT OR IGNORE INTO content_vectors_lsh(hash, band, bucket) VALUES(?1, ?2, ?3)",
                        params![hash, *band as i32, *bucket],
                    )
                    .map_err(sqlite_error)?;
                }
                report.embedded += 1;
            }

            tx.commit().map_err(sqlite_error)?;
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
            let llm_cache_rows = conn
                .execute("DELETE FROM llm_cache", [])
                .map_err(sqlite_error)?;
            let inactive_documents = conn
                .execute("DELETE FROM documents WHERE active = 0", [])
                .map_err(sqlite_error)?;
            let orphaned_content = conn
                .execute(
                    "DELETE FROM content WHERE hash NOT IN (SELECT DISTINCT hash FROM documents)",
                    [],
                )
                .map_err(sqlite_error)?;
            let orphaned_vectors = conn
                .execute(
                    "DELETE FROM content_vectors_native WHERE hash NOT IN (SELECT DISTINCT hash FROM content)",
                    [],
                )
                .map_err(sqlite_error)?;
            conn.execute(
                "DELETE FROM content_vectors_lsh WHERE hash NOT IN (SELECT DISTINCT hash FROM content_vectors_native)",
                [],
            )
            .map_err(sqlite_error)?;

            Ok(CleanupReport {
                llm_cache_rows,
                inactive_documents,
                orphaned_content,
                orphaned_vectors,
            })
        })?;

        let _ = self.with_conn(|conn| {
            conn.execute_batch("VACUUM").map_err(sqlite_error)?;
            Ok(())
        });

        Ok(report)
    }
}

fn explicit_backend(cfg: &RepoQmdConfig, dot: &BTreeMap<String, String>) -> Option<QmdBackendKind> {
    let raw = get_dotenv_or_env(dot, "QMD_BACKEND").or_else(|| cfg.database.backend.clone())?;
    parse_backend_kind(&raw)
}

fn parse_backend_kind(raw: &str) -> Option<QmdBackendKind> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "postgres" | "postgresql" | "pg" => Some(QmdBackendKind::Postgres),
        "sqlite" | "sqlite3" => Some(QmdBackendKind::Sqlite),
        _ => None,
    }
}

fn has_explicit_postgres_config(cfg: &RepoQmdConfig, dot: &BTreeMap<String, String>) -> bool {
    cfg.database.url.as_deref().is_some_and(non_empty)
        || get_dotenv_or_env(dot, "QMD_DATABASE_URL")
            .as_deref()
            .is_some_and(non_empty)
        || get_dotenv_or_env(dot, "DATABASE_URL")
            .as_deref()
            .is_some_and(non_empty)
}

fn discover_git_root(start: &Path) -> Option<PathBuf> {
    let mut cursor = start.to_path_buf();
    loop {
        let dir = cursor.join(".git");
        if dir.exists() {
            return Some(cursor);
        }
        if !cursor.pop() {
            break;
        }
    }
    None
}

fn find_existing_sqlite_path(
    index_name: &str,
    cwd: &Path,
    git_root: Option<&Path>,
) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    candidates.push(cwd.to_path_buf());
    if let Some(root) = git_root
        && root != cwd
    {
        candidates.push(root.to_path_buf());
    }

    let index = sanitize_index_name(index_name);
    for base in candidates {
        let qmd_dir = base.join(".litho").join("qmd");
        let precise = qmd_dir.join(format!("{index}.sqlite3"));
        if precise.exists() {
            return Some(precise);
        }
        if let Ok(entries) = fs::read_dir(&qmd_dir) {
            let mut known = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.extension()
                        .and_then(|e| e.to_str())
                        .map(|ext| {
                            ext.eq_ignore_ascii_case("sqlite")
                                || ext.eq_ignore_ascii_case("sqlite3")
                                || ext.eq_ignore_ascii_case("db")
                        })
                        .unwrap_or(false)
                })
                .collect::<Vec<_>>();
            known.sort();
            if let Some(first) = known.into_iter().next() {
                return Some(first);
            }
        }
    }
    None
}

fn default_sqlite_path(index_name: &str, cwd: &Path, git_root: Option<&Path>) -> PathBuf {
    let base = git_root.unwrap_or(cwd);
    base.join(".litho")
        .join("qmd")
        .join(format!("{}.sqlite3", sanitize_index_name(index_name)))
}

fn non_empty(value: &str) -> bool {
    !value.trim().is_empty()
}

pub(super) fn initialize_sqlite_schema(conn: &Connection) -> QmdResult<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS content (
            hash TEXT PRIMARY KEY,
            doc TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS documents (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            collection TEXT NOT NULL,
            path TEXT NOT NULL,
            title TEXT NOT NULL,
            hash TEXT NOT NULL REFERENCES content(hash) ON DELETE CASCADE,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            modified_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            active INTEGER NOT NULL DEFAULT 1,
            search TEXT NOT NULL DEFAULT '',
            UNIQUE(collection, path)
        );

        CREATE INDEX IF NOT EXISTS idx_documents_collection ON documents(collection, active);
        CREATE INDEX IF NOT EXISTS idx_documents_hash ON documents(hash);
        CREATE INDEX IF NOT EXISTS idx_documents_path ON documents(path, active);
        CREATE INDEX IF NOT EXISTS idx_documents_display_path ON documents(collection, path);

        CREATE TABLE IF NOT EXISTS llm_cache (
            hash TEXT PRIMARY KEY,
            result TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS content_vectors_native (
            hash TEXT PRIMARY KEY,
            dim INTEGER NOT NULL,
            qvec BLOB NOT NULL,
            model TEXT NOT NULL,
            embedded_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_content_vectors_native_model ON content_vectors_native(model);

        CREATE TABLE IF NOT EXISTS content_vectors_lsh (
            hash TEXT NOT NULL,
            band INTEGER NOT NULL,
            bucket INTEGER NOT NULL,
            PRIMARY KEY(hash, band, bucket)
        );
        CREATE INDEX IF NOT EXISTS idx_content_vectors_lsh_lookup ON content_vectors_lsh(band, bucket);
        "#,
    )
    .map_err(sqlite_error)
}

pub(super) fn get_document_by_id_sqlite(
    conn: &mut Connection,
    id: i64,
) -> QmdResult<Option<StoredDocument>> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT d.collection, d.path, d.title, c.doc
            FROM documents d
            JOIN content c ON c.hash = d.hash
            WHERE d.id = ?1 AND d.active = 1
            "#,
        )
        .map_err(sqlite_error)?;

    match stmt.query_row(params![id], |row| {
        Ok(StoredDocument {
            collection: row.get(0)?,
            path: row.get(1)?,
            title: row.get(2)?,
            body: row.get(3)?,
            default_from_line: None,
        })
    }) {
        Ok(doc) => Ok(Some(doc)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(err) => Err(sqlite_error(err)),
    }
}

pub(super) fn get_document_by_collection_path_sqlite(
    conn: &mut Connection,
    collection: &str,
    path: &str,
) -> QmdResult<Option<StoredDocument>> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT d.collection, d.path, d.title, c.doc
            FROM documents d
            JOIN content c ON c.hash = d.hash
            WHERE d.collection = ?1 AND d.path = ?2 AND d.active = 1
            "#,
        )
        .map_err(sqlite_error)?;

    match stmt.query_row(params![collection, path], |row| {
        Ok(StoredDocument {
            collection: row.get(0)?,
            path: row.get(1)?,
            title: row.get(2)?,
            body: row.get(3)?,
            default_from_line: None,
        })
    }) {
        Ok(doc) => Ok(Some(doc)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(err) => Err(sqlite_error(err)),
    }
}

pub(super) fn get_document_by_suffix_sqlite(
    conn: &mut Connection,
    lookup: &str,
) -> QmdResult<Option<StoredDocument>> {
    let suffix = format!("%{lookup}").to_ascii_lowercase();
    let mut stmt = conn
        .prepare(
            r#"
            SELECT d.collection, d.path, d.title, c.doc
            FROM documents d
            JOIN content c ON c.hash = d.hash
            WHERE d.active = 1
            AND lower(d.collection || '/' || d.path) LIKE ?1
            ORDER BY LENGTH(d.path) ASC
            LIMIT 1
            "#,
        )
        .map_err(sqlite_error)?;

    match stmt.query_row(params![suffix], |row| {
        Ok(StoredDocument {
            collection: row.get(0)?,
            path: row.get(1)?,
            title: row.get(2)?,
            body: row.get(3)?,
            default_from_line: None,
        })
    }) {
        Ok(doc) => Ok(Some(doc)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(err) => Err(sqlite_error(err)),
    }
}

fn run_sqlite_lexical_scan(
    conn: &mut Connection,
    collection: Option<&str>,
) -> QmdResult<Vec<FtsRow>> {
    let mut rows_out = Vec::new();
    if let Some(collection) = collection {
        let mut stmt = conn
            .prepare(
                r#"
                SELECT d.id, d.hash, d.collection, d.path, d.title, c.doc
                FROM documents d
                JOIN content c ON c.hash = d.hash
                WHERE d.active = 1 AND d.collection = ?1
                LIMIT 5000
                "#,
            )
            .map_err(sqlite_error)?;
        let rows = stmt
            .query_map(params![collection], |row| {
                Ok(FtsRow {
                    id: row.get(0)?,
                    hash: row.get(1)?,
                    collection: row.get(2)?,
                    path: row.get(3)?,
                    title: row.get(4)?,
                    body: row.get(5)?,
                    rank: 0.0,
                })
            })
            .map_err(sqlite_error)?;
        for row in rows {
            rows_out.push(row.map_err(sqlite_error)?);
        }
    } else {
        let mut stmt = conn
            .prepare(
                r#"
                SELECT d.id, d.hash, d.collection, d.path, d.title, c.doc
                FROM documents d
                JOIN content c ON c.hash = d.hash
                WHERE d.active = 1
                LIMIT 5000
                "#,
            )
            .map_err(sqlite_error)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(FtsRow {
                    id: row.get(0)?,
                    hash: row.get(1)?,
                    collection: row.get(2)?,
                    path: row.get(3)?,
                    title: row.get(4)?,
                    body: row.get(5)?,
                    rank: 0.0,
                })
            })
            .map_err(sqlite_error)?;
        for row in rows {
            rows_out.push(row.map_err(sqlite_error)?);
        }
    }

    Ok(rows_out)
}

fn run_sqlite_vector_scan_all(
    conn: &mut Connection,
    collection: Option<&str>,
) -> QmdResult<Vec<VectorRow>> {
    let mut out = Vec::new();
    if let Some(collection) = collection {
        let mut stmt = conn
            .prepare(
                r#"
                SELECT d.id, d.hash, d.collection, d.path, d.title, c.doc, v.qvec
                FROM documents d
                JOIN content c ON c.hash = d.hash
                JOIN content_vectors_native v ON v.hash = d.hash
                WHERE d.active = 1 AND d.collection = ?1
                LIMIT 4000
                "#,
            )
            .map_err(sqlite_error)?;
        let rows = stmt
            .query_map(params![collection], |row| {
                Ok(VectorRow {
                    id: row.get(0)?,
                    hash: row.get(1)?,
                    collection: row.get(2)?,
                    path: row.get(3)?,
                    title: row.get(4)?,
                    body: row.get(5)?,
                    qvec_blob: row.get(6)?,
                })
            })
            .map_err(sqlite_error)?;
        for row in rows {
            out.push(row.map_err(sqlite_error)?);
        }
    } else {
        let mut stmt = conn
            .prepare(
                r#"
                SELECT d.id, d.hash, d.collection, d.path, d.title, c.doc, v.qvec
                FROM documents d
                JOIN content c ON c.hash = d.hash
                JOIN content_vectors_native v ON v.hash = d.hash
                WHERE d.active = 1
                LIMIT 4000
                "#,
            )
            .map_err(sqlite_error)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(VectorRow {
                    id: row.get(0)?,
                    hash: row.get(1)?,
                    collection: row.get(2)?,
                    path: row.get(3)?,
                    title: row.get(4)?,
                    body: row.get(5)?,
                    qvec_blob: row.get(6)?,
                })
            })
            .map_err(sqlite_error)?;
        for row in rows {
            out.push(row.map_err(sqlite_error)?);
        }
    }
    Ok(out)
}

pub(super) fn sqlite_error(err: rusqlite::Error) -> QmdError {
    QmdError::Internal(format!("sqlite error: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_backend_kind_is_strict() {
        assert_eq!(
            parse_backend_kind("postgres"),
            Some(QmdBackendKind::Postgres)
        );
        assert_eq!(parse_backend_kind("sqlite3"), Some(QmdBackendKind::Sqlite));
        assert_eq!(parse_backend_kind("unknown"), None);
    }

    #[test]
    fn default_sqlite_path_prefers_git_root() {
        let cwd = PathBuf::from("C:/tmp/work/repo/sub");
        let root = PathBuf::from("C:/tmp/work/repo");
        let path = default_sqlite_path("Index-A", &cwd, Some(&root));
        let normalized = path.to_string_lossy().replace('\\', "/");
        assert!(normalized.ends_with("repo/.litho/qmd/index_a.sqlite3"));
    }

    #[test]
    fn backend_resolution_prefers_explicit_backend() {
        let mut dot = BTreeMap::new();
        dot.insert("QMD_BACKEND".to_string(), "sqlite".to_string());

        let cfg = RepoQmdConfig::default();
        let cwd = PathBuf::from("C:/tmp");
        assert_eq!(
            resolve_backend_kind_for("index", &cfg, &dot, &cwd, None),
            QmdBackendKind::Sqlite
        );
    }

    #[test]
    fn backend_resolution_prefers_postgres_url_when_set() {
        let mut dot = BTreeMap::new();
        dot.insert(
            "QMD_DATABASE_URL".to_string(),
            "postgresql://postgres:postgres@127.0.0.1:5432/qmd".to_string(),
        );

        let cfg = RepoQmdConfig::default();
        let cwd = PathBuf::from("C:/tmp");
        assert_eq!(
            resolve_backend_kind_for("index", &cfg, &dot, &cwd, None),
            QmdBackendKind::Postgres
        );
    }

    #[test]
    fn backend_resolution_prefers_repo_local_sqlite_default_over_postgres_url() {
        let mut dot = BTreeMap::new();
        dot.insert(
            "QMD_DATABASE_URL".to_string(),
            "postgresql://postgres:postgres@127.0.0.1:5432/qmd".to_string(),
        );

        let cfg = RepoQmdConfig::default();
        let cwd = PathBuf::from("C:/tmp/repo");
        assert_eq!(
            resolve_backend_kind_for("index", &cfg, &dot, &cwd, Some(Path::new("C:/tmp/repo"))),
            QmdBackendKind::Sqlite
        );
    }
}
