use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Memory metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryMetadata {
    pub created_at: DateTime<Utc>,
    pub last_updated: DateTime<Utc>,
    pub access_counts: HashMap<String, u64>,
    pub data_sizes: HashMap<String, usize>,
    pub total_size: usize,
}

impl Default for MemoryMetadata {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryMetadata {
    pub fn new() -> Self {
        Self {
            created_at: Utc::now(),
            last_updated: Utc::now(),
            access_counts: HashMap::new(),
            data_sizes: HashMap::new(),
            total_size: 0,
        }
    }
}

/// Unified memory manager
#[derive(Debug)]
pub struct Memory {
    data: HashMap<String, Value>,
    metadata: MemoryMetadata,
}

impl Default for Memory {
    fn default() -> Self {
        Self::new()
    }
}

impl Memory {
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
            metadata: MemoryMetadata::new(),
        }
    }

    /// Store data to specified scope and key
    pub fn store<T>(&mut self, scope: &str, key: &str, data: T) -> Result<()>
    where
        T: Serialize,
    {
        let full_key = format!("{}:{}", scope, key);
        let serialized = serde_json::to_value(data)?;

        // Calculate data size
        let data_size = serialized.to_string().len();

        // Update metadata
        if let Some(old_size) = self.metadata.data_sizes.get(&full_key) {
            self.metadata.total_size -= old_size;
        }
        self.metadata.data_sizes.insert(full_key.clone(), data_size);
        self.metadata.total_size += data_size;
        self.metadata.last_updated = Utc::now();

        self.data.insert(full_key, serialized);
        Ok(())
    }

    /// Get data from specified scope and key
    pub fn get<T>(&mut self, scope: &str, key: &str) -> Option<T>
    where
        T: for<'a> Deserialize<'a>,
    {
        let full_key = format!("{}:{}", scope, key);

        // Update access count
        *self
            .metadata
            .access_counts
            .entry(full_key.clone())
            .or_insert(0) += 1;

        self.data
            .get(&full_key)
            .and_then(|value| serde_json::from_value(value.clone()).ok())
    }

    /// List all keys in the specified scope
    pub fn list_keys(&self, scope: &str) -> Vec<String> {
        let prefix = format!("{}:", scope);
        self.data
            .keys()
            .filter(|key| key.starts_with(&prefix))
            .map(|key| key[prefix.len()..].to_string())
            .collect()
    }

    /// Check if specified data exists
    pub fn has_data(&self, scope: &str, key: &str) -> bool {
        let full_key = format!("{}:{}", scope, key);
        self.data.contains_key(&full_key)
    }

    /// Get memory usage statistics
    pub fn get_usage_stats(&self) -> HashMap<String, usize> {
        let mut stats = HashMap::new();

        for (key, size) in &self.metadata.data_sizes {
            let scope = key.split(':').next().unwrap_or("unknown").to_string();
            *stats.entry(scope).or_insert(0) += size;
        }

        stats
    }

    /// Append a learned inference to the specified scope.
    ///
    /// Inferences are accumulated as a `Vec<String>` under the key `{scope}:_inferences`.
    /// Multiple agents can append independently; downstream agents can read the
    /// accumulated list to inform their processing.
    ///
    /// # Examples
    ///
    /// ```
    /// use litho_generator::memory::Memory;
    ///
    /// let mut mem = Memory::new();
    /// mem.append_inference("research", "Module A depends on Module B".to_string());
    /// assert_eq!(mem.get_inferences("research").len(), 1);
    /// ```
    #[allow(dead_code)]
    pub fn append_inference(&mut self, scope: &str, inference: String) {
        let key = format!("{}:_inferences", scope);
        let mut inferences: Vec<String> = self
            .data
            .get(&key)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        inferences.push(inference);

        let serialized = serde_json::to_value(&inferences).unwrap_or_default();
        let data_size = serialized.to_string().len();

        if let Some(old_size) = self.metadata.data_sizes.get(&key) {
            self.metadata.total_size -= old_size;
        }
        self.metadata.data_sizes.insert(key.clone(), data_size);
        self.metadata.total_size += data_size;
        self.metadata.last_updated = Utc::now();

        self.data.insert(key, serialized);
    }

    /// Get all accumulated inferences for a scope.
    ///
    /// Returns an empty `Vec` if no inferences have been appended to the scope.
    ///
    /// # Examples
    ///
    /// ```
    /// use litho_generator::memory::Memory;
    ///
    /// let mem = Memory::new();
    /// assert!(mem.get_inferences("nonexistent").is_empty());
    /// ```
    #[allow(dead_code)]
    pub fn get_inferences(&self, scope: &str) -> Vec<String> {
        let key = format!("{}:_inferences", scope);
        self.data
            .get(&key)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default()
    }

    /// Produce a compressed digest of all data in the specified scope.
    ///
    /// The digest is a summary string listing all keys and a truncated preview
    /// of each value (first 200 chars). This is used at stage boundaries to
    /// create a compressed knowledge summary for downstream context injection.
    /// The `_inferences` key is listed separately at the end of the digest.
    ///
    /// # Examples
    ///
    /// ```
    /// use litho_generator::memory::Memory;
    ///
    /// let mut mem = Memory::new();
    /// mem.store("research", "system_context", "Modular architecture").unwrap();
    /// let digest = mem.digest("research");
    /// assert!(digest.contains("Memory Digest: research"));
    /// assert!(digest.contains("system_context"));
    /// ```
    pub fn digest(&self, scope: &str) -> String {
        let prefix = format!("{}:", scope);
        let mut digest = format!("=== Memory Digest: {} ===\n", scope);

        let mut keys: Vec<&String> = self
            .data
            .keys()
            .filter(|k| k.starts_with(&prefix) && !k.ends_with(":_inferences"))
            .collect();
        keys.sort();

        for key in &keys {
            let short_key = &key[prefix.len()..];
            let preview = self
                .data
                .get(*key)
                .map(|v| {
                    let s = v.to_string();
                    if s.len() > 200 {
                        format!("{}...", &s[..200])
                    } else {
                        s
                    }
                })
                .unwrap_or_default();
            digest.push_str(&format!("- {}: {}\n", short_key, preview));
        }

        // Append inferences if any
        let inferences = self.get_inferences(scope);
        if !inferences.is_empty() {
            digest.push_str(&format!("\nLearned Facts ({}):\n", inferences.len()));
            for (i, inf) in inferences.iter().enumerate().take(20) {
                digest.push_str(&format!("  {}. {}\n", i + 1, inf));
            }
            if inferences.len() > 20 {
                digest.push_str(&format!("  ... and {} more\n", inferences.len() - 20));
            }
        }

        digest
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_append_and_get_inferences() {
        let mut mem = Memory::new();
        mem.append_inference("research", "Module A depends on Module B".to_string());
        mem.append_inference(
            "research",
            "The project uses async/await extensively".to_string(),
        );

        let inferences = mem.get_inferences("research");
        assert_eq!(inferences.len(), 2);
        assert_eq!(inferences[0], "Module A depends on Module B");
        assert_eq!(inferences[1], "The project uses async/await extensively");
    }

    #[test]
    fn test_get_inferences_empty_scope() {
        let mem = Memory::new();
        let inferences = mem.get_inferences("nonexistent");
        assert!(inferences.is_empty());
    }

    #[test]
    fn test_digest_empty_scope() {
        let mem = Memory::new();
        let digest = mem.digest("empty");
        assert!(digest.contains("Memory Digest: empty"));
        assert!(!digest.contains("Learned Facts")); // No inferences
    }

    #[test]
    fn test_digest_with_data_and_inferences() {
        let mut mem = Memory::new();
        mem.store(
            "research",
            "system_context",
            "Large codebase with modular architecture",
        )
        .unwrap();
        mem.store(
            "research",
            "key_modules",
            serde_json::json!(["auth", "api", "db"]),
        )
        .unwrap();
        mem.append_inference("research", "Auth module is the most critical".to_string());

        let digest = mem.digest("research");
        assert!(digest.contains("Memory Digest: research"));
        assert!(digest.contains("system_context"));
        assert!(digest.contains("key_modules"));
        assert!(digest.contains("Learned Facts (1)"));
        assert!(digest.contains("Auth module is the most critical"));
    }

    #[test]
    fn test_digest_truncates_long_values() {
        let mut mem = Memory::new();
        let long_value = "x".repeat(500);
        mem.store("test", "long_key", long_value).unwrap();

        let digest = mem.digest("test");
        assert!(digest.contains("...")); // Should be truncated
    }

    #[test]
    fn test_inferences_isolated_by_scope() {
        let mut mem = Memory::new();
        mem.append_inference("research", "Research fact".to_string());
        mem.append_inference("compose", "Compose fact".to_string());

        assert_eq!(mem.get_inferences("research").len(), 1);
        assert_eq!(mem.get_inferences("compose").len(), 1);
        assert_eq!(mem.get_inferences("research")[0], "Research fact");
        assert_eq!(mem.get_inferences("compose")[0], "Compose fact");
    }

    #[test]
    fn test_memory_metadata_updated_on_inference() {
        let mut mem = Memory::new();
        let initial_size = mem.metadata.total_size;
        mem.append_inference("test", "A learned fact".to_string());
        assert!(mem.metadata.total_size > initial_size);
    }
}
