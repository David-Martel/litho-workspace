use anyhow::Result;
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::fs;

use crate::config::CacheConfig;
use crate::i18n::TargetLanguage;
use crate::llm::client::types::TokenUsage;

pub mod performance_monitor;
pub use performance_monitor::{CachePerformanceMonitor, CachePerformanceReport};

/// Cache manager
pub struct CacheManager {
    config: CacheConfig,
    performance_monitor: CachePerformanceMonitor,
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
        Self {
            config,
            performance_monitor: CachePerformanceMonitor::new(target_language),
        }
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

        if !fs::try_exists(&cache_path).await.unwrap_or(false) {
            self.performance_monitor.record_cache_miss(category);
            return Ok(None);
        }

        match fs::read_to_string(&cache_path).await {
            Ok(content) => {
                match serde_json::from_str::<CacheEntry<T>>(&content) {
                    Ok(entry) => {
                        if self.is_expired(entry.timestamp) {
                            // Delete expired cache
                            let _ = fs::remove_file(&cache_path).await;
                            self.performance_monitor.record_cache_miss(category);
                            return Ok(None);
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
                        self.performance_monitor.record_cache_error(
                            category,
                            &format!("Deserialization failed: {}", e),
                        );
                        Ok(None)
                    }
                }
            }
            Err(e) => {
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
            prompt_hash: hash,
            token_usage: Some(token_usage),
            model_name: None,
        };

        match serde_json::to_string_pretty(&entry) {
            Ok(content) => match fs::write(&cache_path, content).await {
                Ok(_) => {
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
            prompt_hash: hash,
            token_usage: None,
            model_name: None,
        };

        match serde_json::to_string_pretty(&entry) {
            Ok(content) => match fs::write(&cache_path, content).await {
                Ok(_) => {
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
                        self.performance_monitor.record_cache_miss(category);
                        return Ok(None);
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
            Ok(content) => match fs::write(&cache_path, content).await {
                Ok(_) => {
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
