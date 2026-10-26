#[cfg(feature = "postgres")]
use crate::PostgresQmdStore;
use crate::{
    AutoQmdStore, CleanupReport, CollectionMutation, CollectionRecord, CollectionUpdateResult,
    ContextMutation, ContextRecord, ContextTarget, DEFAULT_INDEX_NAME, DocumentContent,
    DocumentRequest, EmbeddingReport, IndexStatus, IngestReport, MultiGetRequest, MultiGetResponse,
    QmdBackendKind, QmdError, QmdResult, QmdStore, SearchHit, SearchOptions, SqliteQmdStore,
    sqlite_impl,
};

impl AutoQmdStore {
    pub fn open_default(index_name: Option<&str>) -> QmdResult<Self> {
        Self::open_with_backend(index_name, None)
    }

    pub fn open_with_backend(
        index_name: Option<&str>,
        backend: Option<QmdBackendKind>,
    ) -> QmdResult<Self> {
        let index = index_name.unwrap_or(DEFAULT_INDEX_NAME);
        let resolved = backend.unwrap_or_else(|| sqlite_impl::resolve_backend_kind(index));
        match resolved {
            #[cfg(feature = "postgres")]
            QmdBackendKind::Postgres => {
                Ok(Self::Postgres(PostgresQmdStore::open_default(Some(index))?))
            }
            #[cfg(not(feature = "postgres"))]
            QmdBackendKind::Postgres => Err(QmdError::InvalidRequest(
                "postgres backend requested but litho-qmd-storage was built without the 'postgres' feature"
                    .to_string(),
            )),
            QmdBackendKind::Sqlite => Ok(Self::Sqlite(SqliteQmdStore::open_default(Some(index))?)),
        }
    }

    pub fn backend_kind(&self) -> QmdBackendKind {
        match self {
            #[cfg(feature = "postgres")]
            AutoQmdStore::Postgres(_) => QmdBackendKind::Postgres,
            AutoQmdStore::Sqlite(_) => QmdBackendKind::Sqlite,
        }
    }
}

impl QmdStore for AutoQmdStore {
    fn status(&self) -> QmdResult<IndexStatus> {
        match self {
            #[cfg(feature = "postgres")]
            AutoQmdStore::Postgres(store) => store.status(),
            AutoQmdStore::Sqlite(store) => store.status(),
        }
    }

    fn search_bm25(&self, query: &str, options: &SearchOptions) -> QmdResult<Vec<SearchHit>> {
        match self {
            #[cfg(feature = "postgres")]
            AutoQmdStore::Postgres(store) => store.search_bm25(query, options),
            AutoQmdStore::Sqlite(store) => store.search_bm25(query, options),
        }
    }

    fn search_vector(&self, query: &str, options: &SearchOptions) -> QmdResult<Vec<SearchHit>> {
        match self {
            #[cfg(feature = "postgres")]
            AutoQmdStore::Postgres(store) => store.search_vector(query, options),
            AutoQmdStore::Sqlite(store) => store.search_vector(query, options),
        }
    }

    fn get_document(&self, request: &DocumentRequest) -> QmdResult<DocumentContent> {
        match self {
            #[cfg(feature = "postgres")]
            AutoQmdStore::Postgres(store) => store.get_document(request),
            AutoQmdStore::Sqlite(store) => store.get_document(request),
        }
    }

    fn multi_get(&self, request: &MultiGetRequest) -> QmdResult<MultiGetResponse> {
        match self {
            #[cfg(feature = "postgres")]
            AutoQmdStore::Postgres(store) => store.multi_get(request),
            AutoQmdStore::Sqlite(store) => store.multi_get(request),
        }
    }

    fn list_files(&self, prefix: Option<&str>) -> QmdResult<Vec<String>> {
        match self {
            #[cfg(feature = "postgres")]
            AutoQmdStore::Postgres(store) => store.list_files(prefix),
            AutoQmdStore::Sqlite(store) => store.list_files(prefix),
        }
    }

    fn list_collections(&self) -> QmdResult<Vec<CollectionRecord>> {
        match self {
            #[cfg(feature = "postgres")]
            AutoQmdStore::Postgres(store) => store.list_collections(),
            AutoQmdStore::Sqlite(store) => store.list_collections(),
        }
    }

    fn add_collection(&self, input: &CollectionMutation) -> QmdResult<()> {
        match self {
            #[cfg(feature = "postgres")]
            AutoQmdStore::Postgres(store) => store.add_collection(input),
            AutoQmdStore::Sqlite(store) => store.add_collection(input),
        }
    }

    fn remove_collection(&self, name: &str) -> QmdResult<bool> {
        match self {
            #[cfg(feature = "postgres")]
            AutoQmdStore::Postgres(store) => store.remove_collection(name),
            AutoQmdStore::Sqlite(store) => store.remove_collection(name),
        }
    }

    fn rename_collection(&self, old_name: &str, new_name: &str) -> QmdResult<()> {
        match self {
            #[cfg(feature = "postgres")]
            AutoQmdStore::Postgres(store) => store.rename_collection(old_name, new_name),
            AutoQmdStore::Sqlite(store) => store.rename_collection(old_name, new_name),
        }
    }

    fn run_collection_updates(&self) -> QmdResult<Vec<CollectionUpdateResult>> {
        match self {
            #[cfg(feature = "postgres")]
            AutoQmdStore::Postgres(store) => store.run_collection_updates(),
            AutoQmdStore::Sqlite(store) => store.run_collection_updates(),
        }
    }

    fn ingest_collections(&self, force: bool) -> QmdResult<IngestReport> {
        match self {
            #[cfg(feature = "postgres")]
            AutoQmdStore::Postgres(store) => store.ingest_collections(force),
            AutoQmdStore::Sqlite(store) => store.ingest_collections(force),
        }
    }

    fn embed_native(&self, force: bool) -> QmdResult<EmbeddingReport> {
        match self {
            #[cfg(feature = "postgres")]
            AutoQmdStore::Postgres(store) => store.embed_native(force),
            AutoQmdStore::Sqlite(store) => store.embed_native(force),
        }
    }

    fn list_contexts(&self) -> QmdResult<Vec<ContextRecord>> {
        match self {
            #[cfg(feature = "postgres")]
            AutoQmdStore::Postgres(store) => store.list_contexts(),
            AutoQmdStore::Sqlite(store) => store.list_contexts(),
        }
    }

    fn add_context(&self, input: &ContextMutation) -> QmdResult<()> {
        match self {
            #[cfg(feature = "postgres")]
            AutoQmdStore::Postgres(store) => store.add_context(input),
            AutoQmdStore::Sqlite(store) => store.add_context(input),
        }
    }

    fn remove_context(&self, target: &ContextTarget) -> QmdResult<bool> {
        match self {
            #[cfg(feature = "postgres")]
            AutoQmdStore::Postgres(store) => store.remove_context(target),
            AutoQmdStore::Sqlite(store) => store.remove_context(target),
        }
    }

    fn cleanup(&self) -> QmdResult<CleanupReport> {
        match self {
            #[cfg(feature = "postgres")]
            AutoQmdStore::Postgres(store) => store.cleanup(),
            AutoQmdStore::Sqlite(store) => store.cleanup(),
        }
    }
}
