//! Chat types — Custom message and tool types replacing rig-core abstractions.
//!
//! These types provide the internal representation for LLM chat interactions,
//! decoupled from any specific LLM framework. The [`ProviderClient`](super::providers::ProviderClient)
//! translates between these types and provider-specific API formats.

use serde::{Deserialize, Serialize};

// ── Chat messages ──────────────────────────────────────────────────────────

/// A message in a chat conversation.
#[derive(Debug, Clone)]
pub enum ChatMessage {
    /// System instruction message.
    System { content: String },
    /// User input message.
    User { content: String },
    /// Assistant response message.
    Assistant { content: Vec<AssistantContent> },
    /// Tool result message (response to a tool call).
    Tool {
        tool_call_id: String,
        content: String,
    },
}

impl ChatMessage {
    pub fn system(content: &str) -> Self {
        ChatMessage::System {
            content: content.to_string(),
        }
    }

    pub fn user(content: &str) -> Self {
        ChatMessage::User {
            content: content.to_string(),
        }
    }

    pub fn assistant_text(text: &str) -> Self {
        ChatMessage::Assistant {
            content: vec![AssistantContent::Text(TextContent {
                text: text.to_string(),
            })],
        }
    }

    pub fn tool_result(tool_call_id: &str, content: &str) -> Self {
        ChatMessage::Tool {
            tool_call_id: tool_call_id.to_string(),
            content: content.to_string(),
        }
    }
}

// ── Assistant content variants ─────────────────────────────────────────────

/// Content within an assistant message.
#[derive(Debug, Clone)]
pub enum AssistantContent {
    /// Text content from the assistant.
    Text(TextContent),
    /// A tool/function call request.
    ToolCall(ToolCallInfo),
    /// Reasoning trace (e.g. from models that expose chain-of-thought).
    Reasoning(ReasoningContent),
}

/// Plain text content.
#[derive(Debug, Clone)]
pub struct TextContent {
    pub text: String,
}

/// Information about a tool call requested by the assistant.
#[derive(Debug, Clone)]
pub struct ToolCallInfo {
    /// Unique ID for this tool call (used to correlate with tool result).
    pub id: String,
    /// The function being called.
    pub function: FunctionCall,
}

/// Function call details within a tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    /// Name of the function to call.
    pub name: String,
    /// JSON-encoded arguments for the function.
    pub arguments: String,
}

/// Reasoning trace content.
#[derive(Debug, Clone)]
pub struct ReasoningContent {
    pub reasoning: Vec<String>,
}

// ── Tool definitions ───────────────────────────────────────────────────────

/// Definition of a tool that an agent can use (provider-agnostic).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Tool choice configuration.
#[derive(Debug, Clone)]
pub enum ToolChoice {
    /// Model can choose whether to use tools.
    Auto,
    /// Model must use at least one tool.
    Required,
    /// Model must not use tools.
    None,
}

// ── Error types ────────────────────────────────────────────────────────────

/// Error type for prompt execution, replacing `rig::completion::PromptError`.
#[derive(Debug)]
pub enum PromptError {
    /// Reached maximum iteration depth in multi-turn dialogue.
    MaxDepthError {
        max_depth: usize,
        chat_history: Vec<ChatMessage>,
        prompt: String,
    },
    /// An error occurred during completion.
    CompletionError(anyhow::Error),
}

impl std::fmt::Display for PromptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PromptError::MaxDepthError { max_depth, .. } => {
                write!(f, "Reached maximum iteration depth: {}", max_depth)
            }
            PromptError::CompletionError(e) => write!(f, "Completion error: {}", e),
        }
    }
}

impl std::error::Error for PromptError {}

// Note: conversion to anyhow::Error is automatic via the Error trait impl above.

// ── Internal API types (OpenAI-compatible) ─────────────────────────────────

/// OpenAI-compatible chat completion request body.
#[derive(Debug, Serialize)]
pub(crate) struct OpenAIRequest {
    pub model: String,
    pub messages: Vec<OpenAIMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<OpenAITool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct OpenAIMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OpenAIToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct OpenAIToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: OpenAIFunctionCall,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct OpenAIFunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct OpenAITool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: OpenAIToolFunction,
}

#[derive(Debug, Serialize)]
pub(crate) struct OpenAIToolFunction {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// OpenAI-compatible chat completion response.
#[derive(Debug, Deserialize)]
pub(crate) struct OpenAIResponse {
    pub choices: Vec<OpenAIChoice>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAIChoice {
    pub message: OpenAIMessage,
}

// ── Internal API types (Anthropic) ─────────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct AnthropicRequest {
    pub model: String,
    pub system: String,
    pub messages: Vec<AnthropicMessage>,
    pub max_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<AnthropicTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<AnthropicToolChoice>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct AnthropicMessage {
    pub role: String,
    pub content: serde_json::Value, // String or array of content blocks
}

#[derive(Debug, Serialize)]
pub(crate) struct AnthropicTool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub(crate) struct AnthropicToolChoice {
    #[serde(rename = "type")]
    pub choice_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AnthropicResponse {
    pub content: Vec<AnthropicContentBlock>,
    #[serde(default)]
    pub stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum AnthropicContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

// ── Internal API types (Gemini) ────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct GeminiRequest {
    pub contents: Vec<GeminiContent>,
    #[serde(rename = "systemInstruction", skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<GeminiContent>,
    #[serde(rename = "generationConfig", skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GeminiGenerationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<GeminiToolDeclaration>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct GeminiContent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub(crate) enum GeminiPart {
    Text {
        text: String,
    },
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: GeminiFunctionCall,
    },
    FunctionResponse {
        #[serde(rename = "functionResponse")]
        function_response: GeminiFunctionResponse,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct GeminiFunctionCall {
    pub name: String,
    pub args: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct GeminiFunctionResponse {
    pub name: String,
    pub response: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub(crate) struct GeminiGenerationConfig {
    #[serde(rename = "maxOutputTokens", skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
}

#[derive(Debug, Serialize)]
pub(crate) struct GeminiToolDeclaration {
    #[serde(rename = "functionDeclarations")]
    pub function_declarations: Vec<GeminiFunctionDecl>,
}

#[derive(Debug, Serialize)]
pub(crate) struct GeminiFunctionDecl {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GeminiResponse {
    pub candidates: Vec<GeminiCandidate>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GeminiCandidate {
    pub content: GeminiContent,
}
