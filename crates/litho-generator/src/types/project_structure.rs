use std::{collections::HashMap, path::PathBuf};

use serde::{de::Deserializer, Deserialize, Serialize};

use crate::types::{DirectoryInfo, FileInfo};

/// Deserializes a `String` that also accepts arrays or objects.
/// Ollama models sometimes return `["a", "b"]` or `{name: "a"}` or `null`
/// where a single `String` is expected. Arrays are joined with ", ".
fn deserialize_string_or_array<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let val = serde_json::Value::deserialize(deserializer)?;
    match val {
        serde_json::Value::String(s) => Ok(s),
        serde_json::Value::Array(arr) => {
            let parts: Vec<String> = arr
                .into_iter()
                .filter_map(|v| match v {
                    serde_json::Value::String(s) => Some(s),
                    serde_json::Value::Object(map) => {
                        map.get("name").and_then(|n| n.as_str()).map(String::from)
                    }
                    _ => Some(v.to_string()),
                })
                .collect();
            Ok(parts.join(", "))
        }
        serde_json::Value::Object(map) => {
            if let Some(name) = map.get("name").and_then(|n| n.as_str()) {
                Ok(name.to_string())
            } else {
                Ok(serde_json::Value::Object(map).to_string())
            }
        }
        serde_json::Value::Null => Ok(String::new()),
        other => Ok(other.to_string()),
    }
}

/// Project structure information
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProjectStructure {
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub project_name: String,
    pub root_path: PathBuf,
    #[serde(default)]
    pub directories: Vec<DirectoryInfo>,
    #[serde(default)]
    pub files: Vec<FileInfo>,
    #[serde(default)]
    pub total_files: usize,
    #[serde(default)]
    pub total_directories: usize,
    #[serde(default)]
    pub file_types: HashMap<String, usize>,
    #[serde(default)]
    pub size_distribution: HashMap<String, usize>,
}
