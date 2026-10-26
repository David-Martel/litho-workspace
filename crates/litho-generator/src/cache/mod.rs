use anyhow::Result;
use md5::{Digest, Md5};
use moka::future::Cache;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::fs;

use crate::config::CacheConfig;
use crate::i18n::TargetLanguage;
use crate::llm::client::types::TokenUsage;

pub mod performance_monitor;
pub mod repo_index;
pub use performance_monitor::{CachePerformanceMonitor, CachePerformanceReport};

/// Cache manager
pub struct CacheManager {
    config: CacheConfig,
    performance_monitor: CachePerformanceMonitor,
    hot_cache: Cache<String, String>,
    sqlite_store: Option<SqliteCacheStore>,
}

#[derive(Debug, Clone)]
struct SqliteCacheStore {
    db_path: PathBuf,
}

impl SqliteCacheStore {
    fn new(db_path: PathBuf) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&db_path)?;
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS cache_entries (
                category TEXT NOT NULL,
                hash TEXT NOT NULL,
                payload TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                PRIMARY KEY(category, hash)
            );
            CREATE INDEX IF NOT EXISTS idx_cache_entries_timestamp ON cache_entries(timestamp);
            ",
        )?;
        Ok(Self { db_path })
    }

    async fn get_payload(&self, category: String, hash: String) -> Result<Option<String>> {
        let db_path = self.db_path.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<String>> {
            let conn = Connection::open(&db_path)?;
            let mut stmt = conn
                .prepare("SELECT payload FROM cache_entries WHERE category = ?1 AND hash = ?2")?;
            let mut rows = stmt.query(params![category, hash])?;
            if let Some(row) = rows.next()? {
                let payload: String = row.get(0)?;
                Ok(Some(payload))
            } else {
                Ok(None)
            }
        })
        .await?
    }

    async fn set_payload(
        &self,
        category: String,
        hash: String,
        payload: String,
        timestamp: i64,
    ) -> Result<()> {
        let db_path = self.db_path.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = Connection::open(&db_path)?;
            conn.execute(
                "INSERT INTO cache_entries(category, hash, payload, timestamp)
                 VALUES(?1, ?2, ?3, ?4)
                 ON CONFLICT(category, hash) DO UPDATE SET
                    payload = excluded.payload,
                    timestamp = excluded.timestamp",
                params![category, hash, payload, timestamp],
            )?;
            Ok(())
        })
        .await??;
        Ok(())
    }

    async fn delete_entry(&self, category: String, hash: String) -> Result<()> {
        let db_path = self.db_path.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = Connection::open(&db_path)?;
            conn.execute(
                "DELETE FROM cache_entries WHERE category = ?1 AND hash = ?2",
                params![category, hash],
            )?;
            Ok(())
        })
        .await??;
        Ok(())
    }
}

/// Cache entry
#[derive(Debug, Serialize, Deserialize)]
pub struct CacheEntry<T> {
    pub data: T,
    pub timestamp: u64,
    /// MD5 hash of the prompt, used for cache key generation and verification
    pub prompt_hash: String,
    /// Token usage information (optional, for accurate statistics)
    pub token_usage: Option<TokenUsage>,
    /// Model name used (optional)
    pub model_name: Option<String>,
}

impl CacheManager {
    pub fn new(config: CacheConfig, target_language: TargetLanguage) -> Self {
        let hot_cache_capacity = config.lru_max_entries.max(1) as u64;
        let sqlite_store = if config.sqlite_enabled {
            let sqlite_path = config
                .sqlite_path
                .clone()
                .unwrap_or_else(|| config.cache_dir.join("cache-index.sqlite3"));
            match SqliteCacheStore::new(sqlite_path) {
                Ok(store) => Some(store),
                Err(err) => {
                    eprintln!("⚠️  Warning: failed to initialize sqlite cache store: {err}");
                    None
                }
            }
        } else {
            None
        };

        Self {
            config,
            performance_monitor: CachePerformanceMonitor::new(target_language),
            hot_cache: Cache::new(hot_cache_capacity),
            sqlite_store,
        }
    }

    fn cache_key(category: &str, hash: &str) -> String {
        format!("{category}:{hash}")
    }

    fn cache_entry_from_payload<T>(&self, payload: &str) -> Result<CacheEntry<T>>
    where
        T: for<'de> Deserialize<'de>,
    {
        Ok(serde_json::from_str::<CacheEntry<T>>(payload)?)
    }

    async fn hot_get(&self, category: &str, hash: &str) -> Option<String> {
        self.hot_cache.get(&Self::cache_key(category, hash)).await
    }

    async fn hot_insert(&self, category: &str, hash: &str, payload: String) {
        self.hot_cache
            .insert(Self::cache_key(category, hash), payload)
            .await;
    }

    async fn hot_remove(&self, category: &str, hash: &str) {
        self.hot_cache
            .invalidate(&Self::cache_key(category, hash))
            .await;
    }

    async fn hot_get_with_loader<F, Fut>(
        &self,
        category: &str,
        hash: &str,
        loader: F,
    ) -> Result<String>
    where
        F: FnOnce() -> Fut + Send,
        Fut: Future<Output = Result<String>> + Send,
    {
        let key = Self::cache_key(category, hash);
        let payload = self
            .hot_cache
            .try_get_with(key, async move { loader().await })
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
        Ok(payload)
    }

    /// Generate MD5 hash of the prompt
    pub fn hash_prompt(&self, prompt: &str) -> String {
        let mut hasher = Md5::new();
        hasher.update(prompt.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Get cache file path
    fn get_cache_path(&self, category: &str, hash: &str) -> PathBuf {
        self.config
            .cache_dir
            .join(category)
            .join(format!("{}.json", hash))
    }

    /// Check if cache is expired
    fn is_expired(&self, timestamp: u64) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let expire_seconds = self.config.expire_hours * 3600;
        now - timestamp > expire_seconds
    }

    /// Get cache
    pub async fn get<T>(&self, category: &str, prompt: &str) -> Result<Option<T>>
    where
        T: for<'de> Deserialize<'de>,
    {
        if !self.config.enabled {
            return Ok(None);
        }

        let hash = self.hash_prompt(prompt);
        let cache_path = self.get_cache_path(category, &hash);

        if let Some(payload) = self.hot_get(category, &hash).await
            && let Ok(entry) = self.cache_entry_from_payload::<T>(&payload)
        {
            if self.is_expired(entry.timestamp) {
                self.hot_remove(category, &hash).await;
            } else {
                let estimated_inference_time = self.estimate_inference_time(&payload);
                if let Some(token_usage) = &entry.token_usage {
                    self.performance_monitor.record_cache_hit(
                        category,
                        estimated_inference_time,
                        token_usage.clone(),
                        "",
                    );
                }
                return Ok(Some(entry.data));
            }
        }

        if let Some(store) = &self.sqlite_store
            && let Ok(Some(payload)) = store.get_payload(category.to_string(), hash.clone()).await
            && let Ok(entry) = self.cache_entry_from_payload::<T>(&payload)
        {
            if self.is_expired(entry.timestamp) {
                let _ = store.delete_entry(category.to_string(), hash.clone()).await;
            } else {
                self.hot_insert(category, &hash, payload.clone()).await;
                let estimated_inference_time = self.estimate_inference_time(&payload);
                if let Some(token_usage) = &entry.token_usage {
                    self.performance_monitor.record_cache_hit(
                        category,
                        estimated_inference_time,
                        token_usage.clone(),
                        "",
                    );
                }
                return Ok(Some(entry.data));
            }
        }

        if !fs::try_exists(&cache_path).await.unwrap_or(false) {
            self.performance_monitor.record_cache_miss(category);
            return Ok(None);
        }

        let read_cache_path = cache_path.clone();
        match self
            .hot_get_with_loader(category, &hash, || async move {
                fs::read_to_string(read_cache_path)
                    .await
                    .map_err(Into::into)
            })
            .await
        {
            Ok(content) => {
                match serde_json::from_str::<CacheEntry<T>>(&content) {
                    Ok(entry) => {
                        if self.is_expired(entry.timestamp) {
                            // Delete expired cache
                            let _ = fs::remove_file(&cache_path).await;
                            if let Some(store) = &self.sqlite_store {
                                let _ =
                                    store.delete_entry(category.to_string(), hash.clone()).await;
                            }
                            self.hot_remove(category, &hash).await;
                            self.performance_monitor.record_cache_miss(category);
                            return Ok(None);
                        }

                        self.hot_insert(category, &hash, content.clone()).await;
                        if let Some(store) = &self.sqlite_store {
                            let _ = store
                                .set_payload(
                                    category.to_string(),
                                    hash.clone(),
                                    content.clone(),
                                    entry.timestamp as i64,
                                )
                                .await;
                        }

                        // Use stored token information for accurate statistics
                        let estimated_inference_time = self.estimate_inference_time(&content);

                        if let Some(token_usage) = &entry.token_usage {
                            // Use stored accurate information
                            self.performance_monitor.record_cache_hit(
                                category,
                                estimated_inference_time,
                                token_usage.clone(),
                                "",
                            );
                        }
                        Ok(Some(entry.data))
                    }
                    Err(e) => {
                        self.hot_remove(category, &hash).await;
                        self.performance_monitor.record_cache_error(
                            category,
                            &format!("Deserialization failed: {}", e),
                        );
                        Ok(None)
                    }
                }
            }
            Err(e) => {
                self.hot_remove(category, &hash).await;
                self.performance_monitor
                    .record_cache_error(category, &format!("Failed to read file: {}", e));
                Ok(None)
            }
        }
    }

    /// Set cache (with token usage information)
    pub async fn set_with_tokens<T>(
        &self,
        category: &str,
        prompt: &str,
        data: T,
        token_usage: TokenUsage,
    ) -> Result<()>
    where
        T: Serialize,
    {
        if !self.config.enabled {
            return Ok(());
        }

        let hash = self.hash_prompt(prompt);
        let cache_path = self.get_cache_path(category, &hash);

        // Ensure directory exists
        if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let entry = CacheEntry {
            data,
            timestamp,
            prompt_hash: hash.clone(),
            token_usage: Some(token_usage),
            model_name: None,
        };

        match serde_json::to_string_pretty(&entry) {
            Ok(content) => match fs::write(&cache_path, &content).await {
                Ok(_) => {
                    self.hot_insert(category, &hash, content.clone()).await;
                    if let Some(store) = &self.sqlite_store {
                        let _ = store
                            .set_payload(
                                category.to_string(),
                                hash.clone(),
                                content.clone(),
                                timestamp as i64,
                            )
                            .await;
                    }
                    self.performance_monitor.record_cache_write(category);
                    Ok(())
                }
                Err(e) => {
                    self.performance_monitor
                        .record_cache_error(category, &format!("Failed to write file: {}", e));
                    Err(e.into())
                }
            },
            Err(e) => {
                self.performance_monitor
                    .record_cache_error(category, &format!("Serialization failed: {}", e));
                Err(e.into())
            }
        }
    }

    /// Get compression result cache
    pub async fn get_compression_cache(
        &self,
        original_content: &str,
        content_type: &str,
    ) -> Result<Option<String>> {
        let cache_key = format!("{}_{}", content_type, self.hash_prompt(original_content));
        self.get::<String>("prompt_compression", &cache_key).await
    }

    /// Set compression result cache
    pub async fn set_compression_cache(
        &self,
        original_content: &str,
        content_type: &str,
        compressed_content: String,
    ) -> Result<()> {
        let cache_key = format!("{}_{}", content_type, self.hash_prompt(original_content));
        self.set("prompt_compression", &cache_key, compressed_content)
            .await
    }
    pub async fn set<T>(&self, category: &str, prompt: &str, data: T) -> Result<()>
    where
        T: Serialize,
    {
        if !self.config.enabled {
            return Ok(());
        }

        let hash = self.hash_prompt(prompt);
        let cache_path = self.get_cache_path(category, &hash);

        // Ensure directory exists
        if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let entry = CacheEntry {
            data,
            timestamp,
            prompt_hash: hash.clone(),
            token_usage: None,
            model_name: None,
        };

        match serde_json::to_string_pretty(&entry) {
            Ok(content) => match fs::write(&cache_path, &content).await {
                Ok(_) => {
                    self.hot_insert(category, &hash, content.clone()).await;
                    if let Some(store) = &self.sqlite_store {
                        let _ = store
                            .set_payload(
                                category.to_string(),
                                hash.clone(),
                                content.clone(),
                                timestamp as i64,
                            )
                            .await;
                    }
                    self.performance_monitor.record_cache_write(category);
                    Ok(())
                }
                Err(e) => {
                    self.performance_monitor
                        .record_cache_error(category, &format!("Failed to write file: {}", e));
                    Err(e.into())
                }
            },
            Err(e) => {
                self.performance_monitor
                    .record_cache_error(category, &format!("Serialization failed: {}", e));
                Err(e.into())
            }
        }
    }

    /// Estimate inference time (based on content complexity)
    fn estimate_inference_time(&self, content: &str) -> Duration {
        // Estimate inference time based on content length
        let content_length = content.len();
        let base_time = 2.0; // Base inference time 2 seconds
        let complexity_factor = (content_length as f64 / 1000.0).min(10.0); // Maximum 10x complexity
        let estimated_seconds = base_time + complexity_factor;
        Duration::from_secs_f64(estimated_seconds)
    }

    /// Compute a BLAKE3 content hash for a source file.
    ///
    /// Unlike prompt-keyed caching (MD5 of full prompt), this produces a hash
    /// that depends only on the source content. Template changes, model changes,
    /// and config changes do NOT invalidate content-hash cached results.
    pub fn content_hash(source: &str) -> String {
        blake3::hash(source.as_bytes()).to_hex().to_string()
    }

    /// Look up a previously cached result by content hash.
    pub async fn get_by_content_hash<T>(
        &self,
        category: &str,
        content_hash: &str,
    ) -> Result<Option<T>>
    where
        T: for<'de> Deserialize<'de>,
    {
        if !self.config.enabled {
            return Ok(None);
        }

        if let Some(payload) = self.hot_get(category, content_hash).await
            && let Ok(entry) = self.cache_entry_from_payload::<T>(&payload)
        {
            if self.is_expired(entry.timestamp) {
                self.hot_remove(category, content_hash).await;
            } else {
                return Ok(Some(entry.data));
            }
        }

        if let Some(store) = &self.sqlite_store
            && let Ok(Some(payload)) = store
                .get_payload(category.to_string(), content_hash.to_string())
                .await
            && let Ok(entry) = self.cache_entry_from_payload::<T>(&payload)
        {
            if self.is_expired(entry.timestamp) {
                let _ = store
                    .delete_entry(category.to_string(), content_hash.to_string())
                    .await;
            } else {
                self.hot_insert(category, content_hash, payload.clone())
                    .await;
                return Ok(Some(entry.data));
            }
        }

        let cache_path = self.get_cache_path(category, content_hash);

        if !fs::try_exists(&cache_path).await.unwrap_or(false) {
            self.performance_monitor.record_cache_miss(category);
            return Ok(None);
        }

        match fs::read_to_string(&cache_path).await {
            Ok(content) => match serde_json::from_str::<CacheEntry<T>>(&content) {
                Ok(entry) => {
                    if self.is_expired(entry.timestamp) {
                        let _ = fs::remove_file(&cache_path).await;
                        if let Some(store) = &self.sqlite_store {
                            let _ = store
                                .delete_entry(category.to_string(), content_hash.to_string())
                                .await;
                        }
                        self.hot_remove(category, content_hash).await;
                        self.performance_monitor.record_cache_miss(category);
                        return Ok(None);
                    }

                    self.hot_insert(category, content_hash, content.clone())
                        .await;
                    if let Some(store) = &self.sqlite_store {
                        let _ = store
                            .set_payload(
                                category.to_string(),
                                content_hash.to_string(),
                                content.clone(),
                                entry.timestamp as i64,
                            )
                            .await;
                    }
                    let estimated_inference_time = self.estimate_inference_time(&content);
                    if let Some(token_usage) = &entry.token_usage {
                        self.performance_monitor.record_cache_hit(
                            category,
                            estimated_inference_time,
                            token_usage.clone(),
                            "",
                        );
                    }
                    Ok(Some(entry.data))
                }
                Err(_) => {
                    self.performance_monitor
                        .record_cache_error(category, "content-hash deserialization failed");
                    Ok(None)
                }
            },
            Err(_) => {
                self.performance_monitor
                    .record_cache_error(category, "content-hash read failed");
                Ok(None)
            }
        }
    }

    /// Store a result keyed by content hash.
    pub async fn set_by_content_hash<T>(
        &self,
        category: &str,
        content_hash: &str,
        data: T,
    ) -> Result<()>
    where
        T: Serialize,
    {
        if !self.config.enabled {
            return Ok(());
        }

        let cache_path = self.get_cache_path(category, content_hash);

        if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let entry = CacheEntry {
            data,
            timestamp,
            prompt_hash: content_hash.to_string(),
            token_usage: None,
            model_name: None,
        };

        match serde_json::to_string_pretty(&entry) {
            Ok(content) => match fs::write(&cache_path, &content).await {
                Ok(_) => {
                    self.hot_insert(category, content_hash, content.clone())
                        .await;
                    if let Some(store) = &self.sqlite_store {
                        let _ = store
                            .set_payload(
                                category.to_string(),
                                content_hash.to_string(),
                                content.clone(),
                                timestamp as i64,
                            )
                            .await;
                    }
                    self.performance_monitor.record_cache_write(category);
                    Ok(())
                }
                Err(e) => {
                    self.performance_monitor
                        .record_cache_error(category, &format!("content-hash write failed: {}", e));
                    Err(e.into())
                }
            },
            Err(e) => {
                self.performance_monitor
                    .record_cache_error(category, &format!("content-hash serialize failed: {}", e));
                Err(e.into())
            }
        }
    }

    /// Generate performance report
    pub fn generate_performance_report(&self) -> CachePerformanceReport {
        self.performance_monitor.generate_report()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tempfile::tempdir;

    #[tokio::test]
    async fn hot_cache_round_trip_preserves_payloads() {
        let dir = tempdir().unwrap();
        let config = CacheConfig {
            enabled: true,
            cache_dir: dir.path().to_path_buf(),
            expire_hours: 24,
            lru_max_entries: 8,
            sqlite_enabled: false,
            sqlite_path: None,
            repo_index_enabled: false,
            repo_index_path: None,
        };
        let cache = CacheManager::new(config, TargetLanguage::default());

        cache
            .set("prompt", "hello", serde_json::json!({"value":"world"}))
            .await
            .unwrap();

        let result: Option<serde_json::Value> = cache.get("prompt", "hello").await.unwrap();
        assert_eq!(result.unwrap()["value"], "world");
    }

    #[tokio::test]
    async fn hot_cache_coalesces_duplicate_loads() {
        let cache: Arc<Cache<String, String>> = Arc::new(Cache::new(8));
        let load_count = Arc::new(AtomicUsize::new(0));

        let mut join_set = tokio::task::JoinSet::new();
        for _ in 0..8 {
            let cache = Arc::clone(&cache);
            let load_count = Arc::clone(&load_count);
            join_set.spawn(async move {
                cache
                    .try_get_with("shared-key".to_string(), async move {
                        load_count.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        Ok::<String, anyhow::Error>("shared-value".to_string())
                    })
                    .await
                    .unwrap()
            });
        }

        while let Some(result) = join_set.join_next().await {
            assert_eq!(result.unwrap(), "shared-value");
        }

        assert_eq!(load_count.load(Ordering::SeqCst), 1);
        assert_eq!(
            cache.get(&"shared-key".to_string()).await,
            Some("shared-value".to_string())
        );
    }
}
