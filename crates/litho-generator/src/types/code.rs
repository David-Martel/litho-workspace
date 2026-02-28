use std::{
    fmt::{Display, Formatter},
    path::PathBuf,
};

use schemars::JsonSchema;
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{SeqAccess, Visitor},
};

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

/// Deserializes a `Vec<String>` that also accepts objects with a `name` field.
/// This handles Ollama models that return `[{name: "foo", interface_type: "fn"}]`
/// where `Vec<String>` is expected.
fn deserialize_strings_or_named<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    struct StringsOrNamedVisitor;

    impl<'de> Visitor<'de> for StringsOrNamedVisitor {
        type Value = Vec<String>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a sequence of strings or objects with a 'name' field")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut out = Vec::new();
            while let Some(val) = seq.next_element::<serde_json::Value>()? {
                match val {
                    serde_json::Value::String(s) => out.push(s),
                    serde_json::Value::Object(map) => {
                        if let Some(name) = map.get("name").and_then(|n| n.as_str()) {
                            out.push(name.to_string());
                        }
                    }
                    _ => {}
                }
            }
            Ok(out)
        }
    }

    deserializer.deserialize_seq(StringsOrNamedVisitor)
}

/// Code basic information
#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema, Default)]
pub struct CodeDossier {
    /// Code file name
    #[serde(
        default,
        alias = "title",
        alias = "label",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub name: String,
    /// File path
    #[serde(default)]
    pub file_path: PathBuf,
    /// Source code summary
    #[schemars(skip)]
    #[serde(default)]
    pub source_summary: String,
    /// Purpose type
    #[serde(default, alias = "type", alias = "kind", alias = "category")]
    pub code_purpose: CodePurpose,
    /// Importance score
    #[serde(default)]
    pub importance_score: f64,
    #[serde(
        default,
        alias = "desc",
        alias = "summary",
        deserialize_with = "deserialize_optional_string_or_array"
    )]
    pub description: Option<String>,
    #[serde(default, deserialize_with = "deserialize_strings_or_named")]
    pub functions: Vec<String>,
    /// Interfaces list
    #[serde(default, deserialize_with = "deserialize_strings_or_named")]
    pub interfaces: Vec<String>,
}

/// Intelligent insight information of code file
#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema, Default)]
pub struct CodeInsight {
    /// Code basic information
    #[serde(default)]
    pub code_dossier: CodeDossier,
    #[serde(
        default,
        alias = "description",
        alias = "desc",
        alias = "summary",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub detailed_description: String,
    /// Responsibilities
    #[serde(default)]
    pub responsibilities: Vec<String>,
    /// Contained interfaces
    #[serde(default)]
    pub interfaces: Vec<InterfaceInfo>,
    /// Dependency information
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    #[serde(default)]
    pub complexity_metrics: CodeComplexity,
}

/// Interface information
#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema, Default)]
pub struct InterfaceInfo {
    #[serde(
        default,
        alias = "title",
        alias = "label",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub name: String,
    #[serde(
        default,
        alias = "type",
        alias = "kind",
        alias = "category",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub interface_type: String, // "function", "method", "class", "trait", etc.
    #[serde(default = "default_visibility")]
    pub visibility: String, // "public", "private", "protected"
    #[serde(default, alias = "args", alias = "params")]
    pub parameters: Vec<ParameterInfo>,
    #[serde(
        default,
        alias = "returns",
        deserialize_with = "deserialize_optional_string_or_array"
    )]
    pub return_type: Option<String>,
    #[serde(
        default,
        alias = "desc",
        alias = "summary",
        deserialize_with = "deserialize_optional_string_or_array"
    )]
    pub description: Option<String>,
}

fn default_dependency_type() -> String {
    "import".to_string()
}

fn default_visibility() -> String {
    "public".to_string()
}

/// Parameter information
#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema, Default)]
pub struct ParameterInfo {
    #[serde(
        default,
        alias = "title",
        alias = "label",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub name: String,
    #[serde(
        default,
        alias = "type",
        alias = "kind",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub param_type: String,
    #[serde(default)]
    pub is_optional: bool,
    #[serde(
        default,
        alias = "desc",
        alias = "summary",
        deserialize_with = "deserialize_optional_string_or_array"
    )]
    pub description: Option<String>,
}

/// Dependency information
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct Dependency {
    #[serde(
        default,
        alias = "title",
        alias = "label",
        alias = "module",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub name: String,
    #[serde(
        default,
        alias = "location",
        alias = "file",
        deserialize_with = "deserialize_optional_string_or_array"
    )]
    pub path: Option<String>,
    #[serde(default)]
    pub is_external: bool,
    #[serde(default)]
    pub line_number: Option<usize>,
    #[serde(
        default = "default_dependency_type",
        alias = "type",
        alias = "kind",
        alias = "category"
    )]
    pub dependency_type: String, // "import", "use", "include", "require", etc.
    #[serde(default, deserialize_with = "deserialize_optional_string_or_array")]
    pub version: Option<String>,
}

impl Display for Dependency {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "(name={}, path={}, is_external={},dependency_type={})",
            self.name,
            self.path.as_deref().unwrap_or_default(),
            self.is_external,
            self.dependency_type
        )
    }
}

/// Component complexity metrics
#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema, Default)]
pub struct CodeComplexity {
    #[serde(default)]
    pub cyclomatic_complexity: f64,
    #[serde(default)]
    pub lines_of_code: usize,
    #[serde(default)]
    pub number_of_functions: usize,
    #[serde(default)]
    pub number_of_classes: usize,
}

/// Code functionality classification enum
#[derive(Debug, Serialize, Clone, PartialEq, Eq, Hash, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum CodePurpose {
    /// Project execution entry
    #[serde(alias = "Project execution entry")]
    Entry,
    /// Intelligent Agent
    #[serde(alias = "Intelligent Agent")]
    Agent,
    /// Frontend UI page
    #[serde(alias = "Frontend UI page")]
    Page,
    /// Frontend UI component
    #[serde(alias = "Frontend UI component")]
    Widget,
    /// Code module for implementing specific logical functionality
    #[serde(
        alias = "feature",
        alias = "specific_feature",
        alias = "specific-feature",
        alias = "Code module for implementing specific logical functionality"
    )]
    SpecificFeature,
    /// Data type or model
    #[serde(alias = "Data type or model")]
    Model,
    /// Program internal interface definition
    #[serde(alias = "Program internal interface definition")]
    Types,
    /// Functional tool code for specific scenarios
    #[serde(alias = "Functional tool code for specific scenarios")]
    Tool,
    /// Common, basic utility functions and classes, providing low-level auxiliary functions unrelated to business logic
    #[serde(
        alias = "Common, basic utility functions and classes, providing low-level auxiliary functions unrelated to business logic"
    )]
    Util,
    /// Configuration
    #[serde(alias = "configuration", alias = "Configuration")]
    Config,
    /// Middleware
    #[serde(alias = "Middleware")]
    Middleware,
    /// Plugin
    #[serde(alias = "Plugin")]
    Plugin,
    /// Router in frontend or backend system
    #[serde(alias = "Router in frontend or backend system")]
    Router,
    /// Database component
    #[serde(alias = "Database component")]
    Database,
    /// Service API for external calls, providing calling capabilities based on HTTP, RPC, IPC and other protocols.
    #[serde(
        alias = "Service API for external calls, providing calling capabilities based on HTTP, RPC, IPC and other protocols."
    )]
    Api,
    /// Controller component in MVC architecture, responsible for handling business logic
    #[serde(
        alias = "Controller component in MVC architecture, responsible for handling business logic"
    )]
    Controller,
    /// Service component in MVC architecture, responsible for handling business rules
    #[serde(
        alias = "Service component in MVC architecture, responsible for handling business rules"
    )]
    Service,
    /// Collection of related code (functions, classes, resources) with clear boundaries and responsibilities
    #[serde(
        alias = "Collection of related code (functions, classes, resources) with clear boundaries and responsibilities"
    )]
    Module,
    /// Dependency library
    #[serde(alias = "library", alias = "package", alias = "Dependency library")]
    Lib,
    /// Test component
    #[serde(alias = "testing", alias = "tests", alias = "Test component")]
    Test,
    /// Documentation component
    #[serde(
        alias = "documentation",
        alias = "docs",
        alias = "Documentation component"
    )]
    Doc,
    /// Data Access Layer component
    #[serde(alias = "Data Access Layer component")]
    Dao,
    /// Context component
    #[serde(alias = "Context component")]
    Context,
    /// command-line interface (CLI) commands or message/request handlers
    #[serde(
        alias = "command-line interface (CLI) commands or message/request handlers",
        alias = "command-line interface (CLI) commands or message/request handlers"
    )]
    Command,
    /// Other uncategorized or unknown
    #[serde(
        alias = "unknown",
        alias = "misc",
        alias = "miscellaneous",
        alias = "Other uncategorized or unknown"
    )]
    #[default]
    Other,
}

impl CodePurpose {
    /// Parse loosely formatted LLM output into a known enum variant.
    pub fn from_loose_label(value: &str) -> Self {
        let raw = value.trim();
        if raw.is_empty() {
            return CodePurpose::Other;
        }
        let normalized = raw
            .to_lowercase()
            .replace(['–', '—', '-'], " ")
            .replace('_', " ");

        let compact: String = normalized.chars().filter(|c| !c.is_whitespace()).collect();
        let has = |needle: &str| normalized.contains(needle);
        let has_compact = |needle: &str| compact.contains(needle);

        if has("project execution entry") || has_compact("entry") || (has("main") && has("entry")) {
            return CodePurpose::Entry;
        }
        if has("intelligent agent") || has("agent") {
            return CodePurpose::Agent;
        }
        if has("frontend ui page") || has("ui page") || has("page") {
            return CodePurpose::Page;
        }
        if has("frontend ui component")
            || has("ui component")
            || (has("component") && (has("ui") || has("frontend") || has("widget")))
            || has("widget")
        {
            return CodePurpose::Widget;
        }
        if has("specific feature")
            || has("specificfeature")
            || has("logical functionality")
            || has("feature")
        {
            return CodePurpose::SpecificFeature;
        }
        if has("data type") || has("model") || has("schema") || has("entity") {
            return CodePurpose::Model;
        }
        if has("interface definition") || has("types") || has("type definition") {
            return CodePurpose::Types;
        }
        if has("tool") || has("utility tool") {
            return CodePurpose::Tool;
        }
        if has("utility") || has("helper") || has("low-level auxiliary") {
            return CodePurpose::Util;
        }
        if has("configuration") || has("config") || has("settings") {
            return CodePurpose::Config;
        }
        if has("middleware") {
            return CodePurpose::Middleware;
        }
        if has("plugin") {
            return CodePurpose::Plugin;
        }
        if has("router") || has("routing") {
            return CodePurpose::Router;
        }
        if has("database") || has("sql") || has("storage") || has("repository") {
            return CodePurpose::Database;
        }
        if has("api") || has("http") || has("rpc") || has("endpoint") || has("service api") {
            return CodePurpose::Api;
        }
        if has("controller") {
            return CodePurpose::Controller;
        }
        if has("service") && !has("service api") && !has("api service") {
            return CodePurpose::Service;
        }
        if has("module") {
            return CodePurpose::Module;
        }
        if has("dependency library") || has("library") || has("third party") || has("package") {
            return CodePurpose::Lib;
        }
        if has("test") || has("spec") {
            return CodePurpose::Test;
        }
        if has("documentation") || has("doc") || has("readme") {
            return CodePurpose::Doc;
        }
        if has("data access layer") || has("dao") || has("repository") || has("persistence") {
            return CodePurpose::Dao;
        }
        if has("context") {
            return CodePurpose::Context;
        }
        if has("command line") || has("cli") || has("handler") || has("command") {
            return CodePurpose::Command;
        }
        if has("other") || has("unknown") || has("misc") {
            return CodePurpose::Other;
        }
        CodePurpose::Other
    }

    /// Get component type display name
    pub fn display_name(&self) -> &'static str {
        match self {
            CodePurpose::Entry => "Project Execution Entry",
            CodePurpose::Agent => "Intelligent Agent",
            CodePurpose::Page => "Frontend UI Page",
            CodePurpose::Widget => "Frontend UI Component",
            CodePurpose::SpecificFeature => "Specific Logic Functionality",
            CodePurpose::Model => "Data Type or Model",
            CodePurpose::Util => "Basic Utility Functions",
            CodePurpose::Tool => "Functional Tool Code for Specific Scenarios",
            CodePurpose::Config => "Configuration",
            CodePurpose::Middleware => "Middleware",
            CodePurpose::Plugin => "Plugin",
            CodePurpose::Router => "Router Component",
            CodePurpose::Database => "Database Component",
            CodePurpose::Api => "Various Interface Definitions",
            CodePurpose::Controller => "Controller Component",
            CodePurpose::Service => "Service Component",
            CodePurpose::Module => "Module Component",
            CodePurpose::Lib => "Dependency Library",
            CodePurpose::Test => "Test Component",
            CodePurpose::Doc => "Documentation Component",
            CodePurpose::Other => "Other Component",
            CodePurpose::Types => "Program Interface Definition",
            CodePurpose::Dao => "Data Access Layer Component",
            CodePurpose::Context => "Context Component",
            CodePurpose::Command => "Command",
        }
    }
}

impl Display for CodePurpose {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

impl<'de> Deserialize<'de> for CodePurpose {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let val = serde_json::Value::deserialize(deserializer)?;
        let candidate = match val {
            serde_json::Value::String(s) => s,
            serde_json::Value::Object(map) => map
                .get("code_purpose")
                .and_then(|v| v.as_str())
                .or_else(|| map.get("purpose").and_then(|v| v.as_str()))
                .or_else(|| map.get("type").and_then(|v| v.as_str()))
                .or_else(|| map.get("name").and_then(|v| v.as_str()))
                .unwrap_or("other")
                .to_string(),
            serde_json::Value::Array(items) => items
                .iter()
                .find_map(|item| item.as_str())
                .unwrap_or("other")
                .to_string(),
            _ => "other".to_string(),
        };
        Ok(CodePurpose::from_loose_label(&candidate))
    }
}

/// Component type mapper, used to map original string types to new enum types
pub struct CodePurposeMapper;

impl CodePurposeMapper {
    /// Intelligent mapping based on file path and name
    pub fn map_by_path_and_name(file_path: &str, file_name: &str) -> CodePurpose {
        let path_lower = file_path.to_lowercase();
        let name_lower = file_name.to_lowercase();

        // Extension-based mapping for SQL-related files
        if name_lower.ends_with(".sqlproj") || name_lower.ends_with(".sql") {
            return CodePurpose::Database;
        }

        // Path-based mapping
        if path_lower.contains("/pages/")
            || path_lower.contains("/views/")
            || path_lower.contains("/screens/")
        {
            return CodePurpose::Page;
        }
        if path_lower.contains("/components/")
            || path_lower.contains("/widgets/")
            || path_lower.contains("/ui/")
        {
            return CodePurpose::Widget;
        }
        if path_lower.contains("/models/")
            || path_lower.contains("/entities/")
            || path_lower.contains("/data/")
        {
            return CodePurpose::Model;
        }
        if path_lower.contains("/utils/")
            || path_lower.contains("/utilities/")
            || path_lower.contains("/helpers/")
        {
            return CodePurpose::Util;
        }
        if path_lower.contains("/config/")
            || path_lower.contains("/configs/")
            || path_lower.contains("/settings/")
        {
            return CodePurpose::Config;
        }
        if path_lower.contains("/middleware/") || path_lower.contains("/middlewares/") {
            return CodePurpose::Middleware;
        }
        if path_lower.contains("/plugin/") {
            return CodePurpose::Plugin;
        }
        if path_lower.contains("/routes/")
            || path_lower.contains("/router/")
            || path_lower.contains("/routing/")
        {
            return CodePurpose::Router;
        }
        if path_lower.contains("/database/")
            || path_lower.contains("/db/")
            || path_lower.contains("/storage/")
        {
            return CodePurpose::Database;
        }
        if path_lower.contains("/dao/")
            || path_lower.contains("/repository/")
            || path_lower.contains("/persistence/")
        {
            return CodePurpose::Dao;
        }
        if path_lower.contains("/context") || path_lower.contains("/ctx/") {
            return CodePurpose::Context;
        }
        if path_lower.contains("/api")
            || path_lower.contains("/endpoint")
            || path_lower.contains("/controller")
            || path_lower.contains("/native_module")
            || path_lower.contains("/bridge")
        {
            return CodePurpose::Api;
        }
        if path_lower.contains("/test/")
            || path_lower.contains("/tests/")
            || path_lower.contains("/__tests__/")
        {
            return CodePurpose::Test;
        }
        if path_lower.contains("/docs/")
            || path_lower.contains("/doc/")
            || path_lower.contains("/documentation/")
        {
            return CodePurpose::Doc;
        }

        // Filename-based mapping
        if name_lower.contains("main") || name_lower.contains("index") || name_lower.contains("app")
        {
            return CodePurpose::Entry;
        }
        if name_lower.contains("page")
            || name_lower.contains("view")
            || name_lower.contains("screen")
        {
            return CodePurpose::Page;
        }
        if name_lower.contains("component") || name_lower.contains("widget") {
            return CodePurpose::Widget;
        }
        if name_lower.contains("model") || name_lower.contains("entity") {
            return CodePurpose::Model;
        }
        if name_lower.contains("util") {
            return CodePurpose::Util;
        }
        if name_lower.contains("config") || name_lower.contains("setting") {
            return CodePurpose::Config;
        }
        if name_lower.contains("middleware") {
            return CodePurpose::Middleware;
        }
        if name_lower.contains("plugin") {
            return CodePurpose::Plugin;
        }
        if name_lower.contains("route") {
            return CodePurpose::Router;
        }
        if name_lower.contains("database") {
            return CodePurpose::Database;
        }
        if name_lower.contains("repository") || name_lower.contains("persistence") {
            return CodePurpose::Dao;
        }
        if name_lower.contains("context") || name_lower.contains("ctx") {
            return CodePurpose::Context;
        }
        if name_lower.contains("api") || name_lower.contains("endpoint") {
            return CodePurpose::Api;
        }
        if name_lower.contains("test") || name_lower.contains("spec") {
            return CodePurpose::Test;
        }
        if name_lower.contains("readme") || name_lower.contains("doc") {
            return CodePurpose::Doc;
        }
        if name_lower.contains("cli") || name_lower.contains("commands") {
            return CodePurpose::Command;
        }

        CodePurpose::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sql_file_classification() {
        // .sqlproj files should always be classified as Database
        assert_eq!(
            CodePurposeMapper::map_by_path_and_name("/src/MyProject.sqlproj", "MyProject.sqlproj"),
            CodePurpose::Database
        );

        // .sql files should always be classified as Database
        assert_eq!(
            CodePurposeMapper::map_by_path_and_name("/src/CreateTable.sql", "CreateTable.sql"),
            CodePurpose::Database
        );

        // Even in root directory
        assert_eq!(
            CodePurposeMapper::map_by_path_and_name("/Schema.sql", "Schema.sql"),
            CodePurpose::Database
        );

        // Even with mixed case
        assert_eq!(
            CodePurposeMapper::map_by_path_and_name(
                "/src/StoredProcedures.SQL",
                "StoredProcedures.SQL"
            ),
            CodePurpose::Database
        );
    }

    #[test]
    fn test_sql_file_in_database_folder() {
        // SQL files in /database/ folder should still be Database
        assert_eq!(
            CodePurposeMapper::map_by_path_and_name("/src/database/schema.sql", "schema.sql"),
            CodePurpose::Database
        );
    }

    #[test]
    fn test_path_based_classification() {
        // Files in /database/ folder
        assert_eq!(
            CodePurposeMapper::map_by_path_and_name("/src/database/connection.cs", "connection.cs"),
            CodePurpose::Database
        );

        // Files in /repository/ folder
        assert_eq!(
            CodePurposeMapper::map_by_path_and_name(
                "/src/repository/UserRepository.cs",
                "UserRepository.cs"
            ),
            CodePurpose::Dao
        );
    }
}
