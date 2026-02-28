use anyhow::Result;
use async_trait::async_trait;

use crate::llm::client::chat_types::ToolDefinition;

pub mod file_explorer;
pub mod file_reader;
pub mod time;

/// Trait for tools that can be used by LLM agents in multi-turn dialogue.
///
/// Replaces `rig::tool::Tool` with a simpler interface that returns JSON strings
/// directly, avoiding generic type parameters on the trait itself.
#[async_trait]
pub trait AgentTool: Send + Sync {
    /// The unique name of this tool.
    fn name(&self) -> &str;

    /// Return the tool definition (name, description, parameter schema).
    fn definition(&self) -> ToolDefinition;

    /// Execute the tool with JSON-encoded arguments and return a JSON-encoded result.
    async fn call_json(&self, arguments: &str) -> Result<String>;
}
