use schemars::JsonSchema;
use serde::{Deserialize, Serialize, de::Deserializer};
use std::fmt::Display;

use crate::i18n::TargetLanguage;

/// Deserializes a `String` that also accepts integers, booleans, arrays, or objects.
/// Ollama models sometimes return `0` or `["a","b"]` where a single `String` is expected.
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

/// Optional variant: returns `None` for null/empty, `Some(String)` otherwise.
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
                serde_json::Value::Object(map) => {
                    if let Some(name) = map.get("name").and_then(|n| n.as_str()) {
                        name.to_string()
                    } else {
                        serde_json::Value::Object(map).to_string()
                    }
                }
                other => other.to_string(),
            };
            Ok(Some(s))
        }
    }
}

/// Agent type enumeration
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentType {
    SystemContextResearcher,
    DomainModulesDetector,
    ArchitectureResearcher,
    WorkflowResearcher,
    KeyModulesInsight,
    BoundaryAnalyzer,
    DatabaseOverviewAnalyzer,
}

impl AgentType {
    /// Get localized display name for the agent type
    pub fn display_name(&self, target_language: &TargetLanguage) -> String {
        match self {
            AgentType::SystemContextResearcher => target_language.msg_agent_type("system_context"),
            AgentType::DomainModulesDetector => target_language.msg_agent_type("domain_modules"),
            AgentType::ArchitectureResearcher => target_language.msg_agent_type("architecture"),
            AgentType::WorkflowResearcher => target_language.msg_agent_type("workflow"),
            AgentType::KeyModulesInsight => target_language.msg_agent_type("key_modules"),
            AgentType::BoundaryAnalyzer => target_language.msg_agent_type("boundary"),
            AgentType::DatabaseOverviewAnalyzer => target_language.msg_agent_type("database"),
        }
    }
}

impl Display for AgentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Use English as default for Display trait (used for keys/internal purposes)
        let str = match self {
            AgentType::SystemContextResearcher => "System Context Research Report",
            AgentType::DomainModulesDetector => "Domain Modules Research Report",
            AgentType::ArchitectureResearcher => "System Architecture Research Report",
            AgentType::WorkflowResearcher => "Workflow Research Report",
            AgentType::KeyModulesInsight => "Key Modules and Components Research Report",
            AgentType::BoundaryAnalyzer => "Boundary Interface Research Report",
            AgentType::DatabaseOverviewAnalyzer => "Database Overview Research Report",
        };
        write!(f, "{}", str)
    }
}

// =========================== Specific Agent Result Types ===========================

/// Project type
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProjectType {
    FrontendApp,
    BackendService,
    FullStackApp,
    ComponentLibrary,
    Framework,
    CLITool,
    MobileApp,
    DesktopApp,
    #[default]
    #[serde(other)]
    Other,
}

/// User persona
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct UserPersona {
    #[serde(
        default,
        alias = "title",
        alias = "label",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub name: String,
    #[serde(
        default,
        alias = "desc",
        alias = "summary",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub description: String,
    #[serde(default, alias = "requirements", alias = "goals")]
    pub needs: Vec<String>,
}

/// External system
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct ExternalSystem {
    #[serde(
        default,
        alias = "title",
        alias = "label",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub name: String,
    #[serde(
        default,
        alias = "desc",
        alias = "summary",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub description: String,
    #[serde(
        default,
        alias = "type",
        alias = "kind",
        alias = "category",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub interaction_type: String,
}

/// System boundary
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct SystemBoundary {
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub scope: String,
    #[serde(default, alias = "includes", alias = "elements", alias = "entries")]
    pub included_components: Vec<String>,
    #[serde(default, alias = "excludes")]
    pub excluded_components: Vec<String>,
}

/// Project objective research result
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct SystemContextReport {
    #[serde(
        default,
        alias = "title",
        alias = "name",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub project_name: String,
    #[serde(
        default,
        alias = "desc",
        alias = "summary",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub project_description: String,
    #[serde(default)]
    pub project_type: ProjectType,
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub business_value: String,
    #[serde(default, alias = "users", alias = "personas")]
    pub target_users: Vec<UserPersona>,
    #[serde(default, alias = "external_dependencies", alias = "integrations")]
    pub external_systems: Vec<ExternalSystem>,
    #[serde(default)]
    pub system_boundary: SystemBoundary,
    #[serde(default)]
    pub confidence_score: f64,
}

/// Sub-module definition - represents specific implementation modules within a larger module
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct SubModule {
    /// Sub-module name, should be concise and clear, reflecting specific functionality
    #[serde(
        default,
        alias = "title",
        alias = "label",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub name: String,
    /// Sub-module function description, explaining the specific role and responsibilities
    #[serde(
        default,
        alias = "desc",
        alias = "summary",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub description: String,
    /// Related code file path list, containing all code files implementing this sub-module's functionality
    #[serde(default, alias = "files", alias = "paths")]
    pub code_paths: Vec<String>,
    /// Core function list, listing the main functions and operations provided by this sub-module
    #[serde(default, alias = "functions", alias = "methods")]
    pub key_functions: Vec<String>,
    /// Importance score (1-10), assessing the importance of this sub-module in the overall system
    #[serde(default)]
    pub importance: f64,
}

/// Functional domain module - represents high-level business domain or functional domain
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct DomainModule {
    /// Domain module name, should reflect high-level business or functional domain
    #[serde(
        default,
        alias = "title",
        alias = "label",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub name: String,
    /// Domain module description, detailing the domain's responsibilities, core value, and role in the system
    #[serde(
        default,
        alias = "desc",
        alias = "summary",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub description: String,
    /// Domain type, identifying the domain's layer in system architecture
    #[serde(
        default,
        alias = "type",
        alias = "kind",
        alias = "category",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub domain_type: String,
    /// Sub-module list, containing all specific implementation modules under this domain
    #[serde(default, alias = "modules", alias = "items", alias = "elements")]
    pub sub_modules: Vec<SubModule>,
    /// Related code file path list, containing all code files implementing this domain module
    #[serde(default, alias = "files", alias = "paths")]
    pub code_paths: Vec<String>,
    /// Domain importance score (1-10)
    #[serde(default)]
    pub importance: f64,
    /// Domain complexity score (1-10)
    #[serde(default)]
    pub complexity: f64,
}

/// Inter-domain relationship - represents dependency and collaboration relationships between different domain modules
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct DomainRelation {
    /// Source domain module name, representing the initiator of the dependency relationship
    #[serde(
        default,
        alias = "source",
        alias = "from",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub from_domain: String,
    /// Target domain module name, representing the receiver of the dependency relationship
    #[serde(
        default,
        alias = "target",
        alias = "to",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub to_domain: String,
    /// Relationship type
    #[serde(
        default,
        alias = "type",
        alias = "kind",
        alias = "category",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub relation_type: String,
    /// Dependency strength (1-10)
    #[serde(default)]
    pub strength: f64,
    /// Relationship description
    #[serde(
        default,
        alias = "desc",
        alias = "summary",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub description: String,
}

/// Process step - represents a single execution step in the workflow
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct BusinessFlowStep {
    /// Step number, indicating the execution order of this step in the overall process
    #[serde(default)]
    pub step: usize,
    /// Involved domain module name, identifying the primary domain executing this step
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub domain_module: String,
    /// Involved sub-module name (optional)
    #[serde(default, deserialize_with = "deserialize_optional_string_or_array")]
    pub sub_module: Option<String>,
    /// Specific operation description
    #[serde(
        default,
        alias = "action",
        alias = "desc",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub operation: String,
    /// Code entry point (optional)
    #[serde(default, deserialize_with = "deserialize_optional_string_or_array")]
    pub code_entry_point: Option<String>,
}

/// Core process - represents key functional scenarios and execution paths in the system
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct BusinessFlow {
    /// Process name
    #[serde(
        default,
        alias = "title",
        alias = "label",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub name: String,
    /// Process description
    #[serde(
        default,
        alias = "desc",
        alias = "summary",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub description: String,
    /// Process step list, steps arranged in execution order
    #[serde(default, alias = "items", alias = "elements", alias = "entries")]
    pub steps: Vec<BusinessFlowStep>,
    /// Process entry point
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub entry_point: String,
    /// Process importance score (1-10)
    #[serde(default)]
    pub importance: f64,
    /// Number of involved domains
    #[serde(default)]
    pub involved_domains_count: usize,
}

/// Core component analysis result
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct KeyModuleReport {
    /// Domain name
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub domain_name: String,
    /// Module name
    #[serde(
        default,
        alias = "name",
        alias = "title",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub module_name: String,
    /// Explain the project's current technical solution
    #[serde(
        default,
        alias = "description",
        alias = "desc",
        alias = "summary",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub module_description: String,
    /// Explain the defined interfaces and interaction methods
    #[serde(
        default,
        alias = "interface",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub interaction: String,
    /// Explain technical details
    #[serde(
        default,
        alias = "details",
        alias = "technical_details",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub implementation: String,
    #[serde(default, alias = "files", alias = "paths")]
    pub associated_files: Vec<String>,
    #[serde(
        default,
        alias = "flowchart",
        alias = "mermaid",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub flowchart_mermaid: String,
    #[serde(
        default,
        alias = "sequence",
        alias = "sequence_diagram",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub sequence_diagram_mermaid: String,
}

/// Domain module analysis result from high-level architecture perspective
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct DomainModulesReport {
    /// Identified domain module list
    #[serde(default, alias = "modules", alias = "items", alias = "elements")]
    pub domain_modules: Vec<DomainModule>,
    /// Inter-domain relationship list
    #[serde(
        default,
        alias = "relations",
        alias = "relationships",
        alias = "dependencies"
    )]
    pub domain_relations: Vec<DomainRelation>,
    /// Core business process list
    #[serde(default, alias = "flows", alias = "workflows", alias = "processes")]
    pub business_flows: Vec<BusinessFlow>,
    /// Architecture layer summary
    #[serde(
        default,
        alias = "summary",
        alias = "desc",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub architecture_summary: String,
    /// Analysis confidence score (1-10)
    #[serde(default)]
    pub confidence_score: f64,
}

/// Workflow research result
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkflowReport {
    /// System main workflow
    pub main_workflow: Workflow,
    /// Other important workflows
    #[serde(default, alias = "workflows", alias = "items", alias = "elements")]
    pub other_important_workflows: Vec<Workflow>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct Workflow {
    #[serde(
        default,
        alias = "title",
        alias = "label",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub name: String,
    #[serde(
        default,
        alias = "desc",
        alias = "summary",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub description: String,
    #[serde(
        default,
        alias = "flowchart",
        alias = "mermaid",
        alias = "diagram",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub flowchart_mermaid: String,
}

/// Boundary interface analysis result
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BoundaryAnalysisReport {
    /// CLI boundary interface
    #[serde(default, alias = "cli", alias = "commands")]
    pub cli_boundaries: Vec<CLIBoundary>,
    /// Network API boundary interface for external invocation
    #[serde(default, alias = "api", alias = "apis", alias = "endpoints")]
    pub api_boundaries: Vec<APIBoundary>,
    /// Page routing
    #[serde(default, alias = "routes", alias = "routing")]
    pub router_boundaries: Vec<RouterBoundary>,
    /// Integration suggestions
    #[serde(default, alias = "suggestions", alias = "integrations")]
    pub integration_suggestions: Vec<IntegrationSuggestion>,
    /// Analysis confidence score (1-10)
    #[serde(default)]
    pub confidence_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct CLIBoundary {
    #[serde(
        default,
        alias = "name",
        alias = "cmd",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub command: String,
    #[serde(
        default,
        alias = "desc",
        alias = "summary",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub description: String,
    #[serde(default, alias = "args", alias = "parameters")]
    pub arguments: Vec<CLIArgument>,
    #[serde(default, alias = "flags")]
    pub options: Vec<CLIOption>,
    #[serde(default, alias = "usage", alias = "samples")]
    pub examples: Vec<String>,
    #[serde(
        default,
        alias = "location",
        alias = "file",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub source_location: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct CLIArgument {
    #[serde(
        default,
        alias = "title",
        alias = "label",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub name: String,
    #[serde(
        default,
        alias = "desc",
        alias = "summary",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub description: String,
    #[serde(default)]
    pub required: bool,
    #[serde(
        default,
        alias = "default",
        deserialize_with = "deserialize_optional_string_or_array"
    )]
    pub default_value: Option<String>,
    #[serde(
        default,
        alias = "type",
        alias = "kind",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub value_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct CLIOption {
    #[serde(
        default,
        alias = "title",
        alias = "label",
        alias = "flag",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub name: String,
    #[serde(
        default,
        alias = "short",
        alias = "alias",
        deserialize_with = "deserialize_optional_string_or_array"
    )]
    pub short_name: Option<String>,
    #[serde(
        default,
        alias = "desc",
        alias = "summary",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub description: String,
    #[serde(default)]
    pub required: bool,
    #[serde(
        default,
        alias = "default",
        deserialize_with = "deserialize_optional_string_or_array"
    )]
    pub default_value: Option<String>,
    #[serde(
        default,
        alias = "type",
        alias = "kind",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub value_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct APIBoundary {
    #[serde(
        default,
        alias = "url",
        alias = "path",
        alias = "route",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub endpoint: String,
    #[serde(
        default,
        alias = "http_method",
        alias = "verb",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub method: String,
    #[serde(
        default,
        alias = "desc",
        alias = "summary",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub description: String,
    #[serde(default, deserialize_with = "deserialize_optional_string_or_array")]
    pub request_format: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string_or_array")]
    pub response_format: Option<String>,
    #[serde(
        default,
        alias = "auth",
        deserialize_with = "deserialize_optional_string_or_array"
    )]
    pub authentication: Option<String>,
    #[serde(
        default,
        alias = "location",
        alias = "file",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub source_location: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct RouterBoundary {
    #[serde(
        default,
        alias = "route",
        alias = "url",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub path: String,
    #[serde(
        default,
        alias = "desc",
        alias = "summary",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub description: String,
    #[serde(
        default,
        alias = "location",
        alias = "file",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub source_location: String,
    #[serde(default, alias = "parameters", alias = "args")]
    pub params: Vec<RouterParam>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct RouterParam {
    #[serde(
        default,
        alias = "name",
        alias = "param",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub key: String,
    #[serde(
        default,
        alias = "type",
        alias = "kind",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub value_type: String,
    #[serde(
        default,
        alias = "desc",
        alias = "summary",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct IntegrationSuggestion {
    #[serde(
        default,
        alias = "type",
        alias = "kind",
        alias = "category",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub integration_type: String,
    #[serde(
        default,
        alias = "desc",
        alias = "summary",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub description: String,
    #[serde(
        default,
        alias = "example",
        alias = "code",
        deserialize_with = "deserialize_string_or_array"
    )]
    pub example_code: String,
    #[serde(
        default,
        alias = "practices",
        alias = "tips",
        alias = "recommendations"
    )]
    pub best_practices: Vec<String>,
}

impl Default for BoundaryAnalysisReport {
    fn default() -> Self {
        Self {
            cli_boundaries: Vec::new(),
            api_boundaries: Vec::new(),
            integration_suggestions: Vec::new(),
            confidence_score: 0.0,
            router_boundaries: Vec::new(),
        }
    }
}

/// Database Overview analysis result
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DatabaseOverviewReport {
    /// Database projects found in the solution
    #[serde(default)]
    pub database_projects: Vec<DatabaseProject>,
    /// All tables discovered across all database projects
    #[serde(default)]
    pub tables: Vec<DatabaseTable>,
    /// All views discovered across all database projects
    #[serde(default)]
    pub views: Vec<DatabaseView>,
    /// All stored procedures discovered across all database projects
    #[serde(default)]
    pub stored_procedures: Vec<StoredProcedure>,
    /// All functions discovered across all database projects
    #[serde(default)]
    pub database_functions: Vec<DatabaseFunction>,
    /// Table relationships (foreign keys, references)
    #[serde(default)]
    pub table_relationships: Vec<TableRelationship>,
    /// Data flow patterns identified
    #[serde(default)]
    pub data_flows: Vec<DataFlow>,
    /// Analysis confidence score (1-10)
    #[serde(default)]
    pub confidence_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DatabaseProject {
    /// Project name (from .sqlproj)
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub name: String,
    /// Project file path
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub project_path: String,
    /// Target database platform (SQL Server, etc.)
    #[serde(default, deserialize_with = "deserialize_optional_string_or_array")]
    pub target_platform: Option<String>,
    /// Number of tables
    #[serde(default)]
    pub table_count: usize,
    /// Number of views
    #[serde(default)]
    pub view_count: usize,
    /// Number of stored procedures
    #[serde(default)]
    pub procedure_count: usize,
    /// Number of functions
    #[serde(default)]
    pub function_count: usize,
    /// Referenced database projects or DACPACs
    #[serde(default)]
    pub references: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DatabaseTable {
    /// Schema name (e.g., dbo)
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub schema: String,
    /// Table name
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub name: String,
    /// Column definitions
    #[serde(default)]
    pub columns: Vec<TableColumn>,
    /// Primary key columns
    #[serde(default)]
    pub primary_key: Vec<String>,
    /// Description/purpose of the table
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub description: String,
    /// Source file path
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub source_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TableColumn {
    /// Column name
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub name: String,
    /// Data type (e.g., INT, NVARCHAR(100))
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub data_type: String,
    /// Whether the column allows NULL
    #[serde(default)]
    pub nullable: bool,
    /// Whether this is an identity/auto-increment column
    #[serde(default)]
    pub is_identity: bool,
    /// Default value if any
    #[serde(default, deserialize_with = "deserialize_optional_string_or_array")]
    pub default_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DatabaseView {
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub schema: String,
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub description: String,
    #[serde(default)]
    pub referenced_tables: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub source_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StoredProcedure {
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub schema: String,
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub name: String,
    #[serde(default)]
    pub parameters: Vec<ProcedureParameter>,
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub description: String,
    #[serde(default)]
    pub referenced_tables: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub source_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProcedureParameter {
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub data_type: String,
    #[serde(default)]
    pub is_optional: bool,
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub direction: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DatabaseFunction {
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub schema: String,
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub function_type: String,
    #[serde(default)]
    pub parameters: Vec<ProcedureParameter>,
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub return_type: String,
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub description: String,
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub source_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TableRelationship {
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub from_table: String,
    #[serde(default)]
    pub from_columns: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub to_table: String,
    #[serde(default)]
    pub to_columns: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub relationship_type: String,
    #[serde(default, deserialize_with = "deserialize_optional_string_or_array")]
    pub constraint_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DataFlow {
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub source: String,
    #[serde(default, deserialize_with = "deserialize_string_or_array")]
    pub destination: String,
    #[serde(default)]
    pub operations: Vec<String>,
    #[serde(default)]
    pub procedures_involved: Vec<String>,
}

impl Default for DatabaseOverviewReport {
    fn default() -> Self {
        Self {
            database_projects: Vec::new(),
            tables: Vec::new(),
            views: Vec::new(),
            stored_procedures: Vec::new(),
            database_functions: Vec::new(),
            table_relationships: Vec::new(),
            data_flows: Vec::new(),
            confidence_score: 0.0,
        }
    }
}

// https://c4model.com/abstractions/software-system
// System name, project's role and value, system type, who is using it, how to use, which external systems it interacts with, diagram
