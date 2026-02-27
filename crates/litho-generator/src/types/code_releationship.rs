use schemars::JsonSchema;
use serde::{Deserialize, Serialize, de::Deserializer};

/// Deserializes a `u8` that may arrive as a float, integer, or numeric string.
///
/// LLMs frequently return importance scores as floats (e.g. `0.8`, `3.5`)
/// even when the schema documents them as integers.  This helper coerces the
/// value safely:
///
/// - Float  → rounded to the nearest integer, then clamped to `[0, 255]`
/// - Integer → used directly (clamped)
/// - String  → parsed as `f64` first, then rounded
/// - Anything else (null, bool, array, object) → returns `0`
fn deserialize_u8_or_float<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: Deserializer<'de>,
{
    let val = serde_json::Value::deserialize(deserializer)?;
    let as_f64 = match &val {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    };
    Ok(as_f64
        .map(|f| f.round().clamp(0.0, 255.0) as u8)
        .unwrap_or(0))
}

/// Deserializes a `String` that also accepts arrays or objects.
/// Ollama models sometimes return `["a", "b"]` or `[{name: "a"}]` or `[]`
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

/// Deserializes an `Option<String>` that also accepts arrays or objects.
fn deserialize_optional_string_or_array<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let val = serde_json::Value::deserialize(deserializer)?;
    match val {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::String(s) if s.is_empty() => Ok(None),
        serde_json::Value::Array(ref arr) if arr.is_empty() => Ok(None),
        _ => {
            // Reuse the same logic as the non-optional version
            let s = match val {
                serde_json::Value::String(s) => s,
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
                    parts.join(", ")
                }
                serde_json::Value::Object(map) => map
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or_default()
                    .to_string(),
                other => other.to_string(),
            };
            Ok(Some(s))
        }
    }
}

/// Streamlined relationship analysis result
#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema, Default)]
pub struct RelationshipAnalysis {
    /// Core dependency relationships (only keep important ones)
    #[serde(default, alias = "dependencies", alias = "relations", alias = "items")]
    pub core_dependencies: Vec<CoreDependency>,

    /// Architecture layer information
    #[serde(default, alias = "layers", alias = "architecture")]
    pub architecture_layers: Vec<ArchitectureLayer>,

    /// Key issues and recommendations
    #[serde(
        default,
        alias = "insights",
        alias = "observations",
        alias = "recommendations"
    )]
    pub key_insights: Vec<String>,
}

/// Core dependency (simplified version)
#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema, Default)]
pub struct CoreDependency {
    /// Source component
    #[serde(
        default,
        alias = "source",
        alias = "origin",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub from: String,

    /// Target component
    #[serde(
        default,
        alias = "target",
        alias = "destination",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub to: String,

    /// Dependency type
    #[serde(default, alias = "type", alias = "kind", alias = "category")]
    pub dependency_type: DependencyType,

    /// Importance score (1-5, only keep important ones).
    ///
    /// LLMs sometimes return a float (e.g. `0.8`) instead of an integer here.
    /// The custom deserializer rounds and clamps the value to `u8` so that
    /// fractional scores from the model do not crash deserialization.
    #[serde(default, deserialize_with = "deserialize_u8_or_float")]
    pub importance: u8,

    /// Brief description
    #[serde(
        default,
        alias = "desc",
        alias = "summary",
        deserialize_with = "deserialize_optional_string_or_array"
    )]
    pub description: Option<String>,
}

/// Architecture layer
#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema, Default)]
pub struct ArchitectureLayer {
    /// Layer name
    #[serde(
        default,
        alias = "title",
        alias = "label",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub name: String,

    /// Components in this layer
    #[serde(default, alias = "items", alias = "elements", alias = "entries")]
    pub components: Vec<String>,

    /// Layer level (smaller number means lower level)
    #[serde(default)]
    pub level: u8,
}

/// Dependency type enumeration
#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
pub enum DependencyType {
    /// Import dependency (use, import statements)
    #[serde(
        alias = "import",
        alias = "Include",
        alias = "include",
        alias = "Header",
        alias = "header",
        alias = "Use",
        alias = "use",
        alias = "Require",
        alias = "require"
    )]
    Import,
    /// Function call dependency
    #[serde(alias = "function_call", alias = "Call", alias = "call")]
    FunctionCall,
    /// Inheritance relationship
    #[serde(alias = "inheritance", alias = "Extends", alias = "extends")]
    Inheritance,
    /// Composition relationship
    #[serde(
        alias = "composition",
        alias = "Contains",
        alias = "contains",
        alias = "Aggregation",
        alias = "aggregation"
    )]
    Composition,
    /// Data flow dependency
    #[serde(alias = "data_flow", alias = "Data", alias = "data")]
    DataFlow,
    /// Module dependency
    #[serde(
        alias = "module",
        alias = "Library",
        alias = "library",
        alias = "Package",
        alias = "package",
        alias = "Dependency",
        alias = "dependency"
    )]
    Module,
    /// Unknown or unrecognized type (catch-all)
    #[serde(other)]
    Other,
}

impl Default for DependencyType {
    fn default() -> Self {
        DependencyType::Import
    }
}

impl DependencyType {
    pub fn as_str(&self) -> &'static str {
        match self {
            DependencyType::Import => "import",
            DependencyType::FunctionCall => "function_call",
            DependencyType::Inheritance => "inheritance",
            DependencyType::Composition => "composition",
            DependencyType::DataFlow => "data_flow",
            DependencyType::Module => "module",
            DependencyType::Other => "other",
        }
    }
}
