//! LLM Provider support module — direct HTTP implementation.
//!
//! Replaces the rig-core 0.23 abstraction with direct `reqwest` calls to
//! OpenAI-compatible, Anthropic, and Gemini APIs.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::{LLMConfig, LLMProvider};
use crate::llm::tools::AgentTool;

use super::chat_types::*;
use super::codex_provider::CodexRsClient;
use super::ollama_extractor::OllamaExtractorWrapper;

// ── ProviderClient ─────────────────────────────────────────────────────────

/// Unified provider client backed by `reqwest`.
#[derive(Clone)]
pub struct ProviderClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    provider: LLMProvider,
    /// Only populated for `LLMProvider::CodexRs`.
    codex_client: Option<CodexRsClient>,
}

impl ProviderClient {
    /// Create a provider client based on configuration.
    pub fn new(config: &LLMConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_seconds))
            .build()
            .context("Failed to build HTTP client")?;

        let codex_client = if config.provider == LLMProvider::CodexRs {
            Some(CodexRsClient::new(
                config.codex_binary_path.as_deref(),
                Some(config.timeout_seconds),
                Some(config.model_powerful.clone()),
                None,
            )?)
        } else {
            None
        };

        Ok(Self {
            http,
            base_url: config.api_base_url.clone(),
            api_key: config.api_key.clone(),
            provider: config.provider.clone(),
            codex_client,
        })
    }

    /// Create an agent (no tools).
    pub fn create_agent(
        &self,
        model: &str,
        system_prompt: &str,
        config: &LLMConfig,
    ) -> ProviderAgent {
        ProviderAgent {
            client: self.clone(),
            model: model.to_string(),
            system_prompt: system_prompt.to_string(),
            max_tokens: config.max_tokens,
            temperature: config.temperature,
            tools: Vec::new(),
            tool_choice: None,
        }
    }

    /// Create an agent with tools for multi-turn dialogue.
    pub fn create_agent_with_tools(
        &self,
        model: &str,
        system_prompt: &str,
        config: &LLMConfig,
        tools: Vec<Arc<dyn AgentTool>>,
    ) -> ProviderAgent {
        ProviderAgent {
            client: self.clone(),
            model: model.to_string(),
            system_prompt: system_prompt.to_string(),
            max_tokens: config.max_tokens,
            temperature: config.temperature,
            tools,
            tool_choice: Some(ToolChoice::Required),
        }
    }

    /// Create an extractor for structured output.
    pub fn create_extractor<T>(
        &self,
        model: &str,
        system_prompt: &str,
        config: &LLMConfig,
    ) -> ProviderExtractor<T>
    where
        T: JsonSchema + for<'a> Deserialize<'a> + Serialize + Send + Sync + 'static,
    {
        match self.provider {
            LLMProvider::Ollama => {
                // Ollama: use prompt-based extraction with JSON parsing cascade
                let agent = self.create_agent(model, system_prompt, config);
                let wrapper = OllamaExtractorWrapper::new(agent, config.retry_attempts);
                ProviderExtractor::Ollama(wrapper)
            }
            LLMProvider::CodexRs => {
                let client = self.codex_client.clone().expect("CodexRs client not set");
                ProviderExtractor::CodexRs {
                    client,
                    model: model.to_string(),
                    system_prompt: system_prompt.to_string(),
                    _phantom: std::marker::PhantomData,
                }
            }
            _ => {
                // All other providers: use function-calling extraction
                ProviderExtractor::FunctionCalling {
                    client: self.clone(),
                    model: model.to_string(),
                    system_prompt: system_prompt.to_string(),
                    max_tokens: config.max_tokens,
                    _phantom: std::marker::PhantomData,
                }
            }
        }
    }

    // ── HTTP dispatch ──────────────────────────────────────────────────

    /// Send a chat completion request and return the parsed response.
    async fn send_completion(
        &self,
        model: &str,
        messages: &[ChatMessage],
        max_tokens: u32,
        temperature: Option<f64>,
        tools: Option<&[ToolDefinition]>,
        tool_choice: Option<&ToolChoice>,
    ) -> Result<CompletionResponse> {
        match self.provider {
            LLMProvider::OpenAI
            | LLMProvider::DeepSeek
            | LLMProvider::Moonshot
            | LLMProvider::Mistral
            | LLMProvider::OpenRouter
            | LLMProvider::Ollama => {
                self.send_openai_compatible(
                    model,
                    messages,
                    max_tokens,
                    temperature,
                    tools,
                    tool_choice,
                )
                .await
            }
            LLMProvider::Anthropic => {
                self.send_anthropic(model, messages, max_tokens, temperature, tools, tool_choice)
                    .await
            }
            LLMProvider::Gemini => {
                self.send_gemini(model, messages, max_tokens, temperature, tools, tool_choice)
                    .await
            }
            LLMProvider::CodexRs => {
                anyhow::bail!("CodexRs uses subprocess, not HTTP completion API")
            }
        }
    }

    // ── OpenAI-compatible path ─────────────────────────────────────────

    async fn send_openai_compatible(
        &self,
        model: &str,
        messages: &[ChatMessage],
        max_tokens: u32,
        temperature: Option<f64>,
        tools: Option<&[ToolDefinition]>,
        tool_choice: Option<&ToolChoice>,
    ) -> Result<CompletionResponse> {
        let api_messages = messages.iter().map(chat_to_openai).collect();

        let api_tools = tools.map(|ts| {
            ts.iter()
                .map(|t| OpenAITool {
                    tool_type: "function".to_string(),
                    function: OpenAIToolFunction {
                        name: t.name.clone(),
                        description: t.description.clone(),
                        parameters: t.parameters.clone(),
                    },
                })
                .collect()
        });

        let api_tool_choice = tool_choice.map(|tc| match tc {
            ToolChoice::_Auto => serde_json::json!("auto"),
            ToolChoice::Required => serde_json::json!("required"),
            ToolChoice::_None => serde_json::json!("none"),
        });

        let request = OpenAIRequest {
            model: model.to_string(),
            messages: api_messages,
            max_tokens: Some(max_tokens as u64),
            temperature,
            tools: api_tools,
            tool_choice: api_tool_choice,
        };

        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .context("Failed to send OpenAI-compatible request")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "OpenAI-compatible API error ({}): {}",
                status,
                body.chars().take(500).collect::<String>()
            );
        }

        let api_resp: OpenAIResponse = resp
            .json()
            .await
            .context("Failed to parse OpenAI-compatible response")?;

        let choice = api_resp
            .choices
            .into_iter()
            .next()
            .context("Empty choices in response")?;

        Ok(openai_to_completion(choice.message))
    }

    // ── Anthropic path ─────────────────────────────────────────────────

    async fn send_anthropic(
        &self,
        model: &str,
        messages: &[ChatMessage],
        max_tokens: u32,
        temperature: Option<f64>,
        tools: Option<&[ToolDefinition]>,
        tool_choice: Option<&ToolChoice>,
    ) -> Result<CompletionResponse> {
        // Extract system message, convert rest to Anthropic format
        let mut system_text = String::new();
        let mut api_messages = Vec::new();

        for msg in messages {
            match msg {
                ChatMessage::System { content } => {
                    system_text = content.clone();
                }
                ChatMessage::User { content } => {
                    api_messages.push(AnthropicMessage {
                        role: "user".to_string(),
                        content: serde_json::Value::String(content.clone()),
                    });
                }
                ChatMessage::Assistant { content } => {
                    let blocks: Vec<serde_json::Value> = content
                        .iter()
                        .filter_map(|c| match c {
                            AssistantContent::Text(t) => Some(serde_json::json!({
                                "type": "text",
                                "text": t.text
                            })),
                            AssistantContent::ToolCall(tc) => Some(serde_json::json!({
                                "type": "tool_use",
                                "id": tc.id,
                                "name": tc.function.name,
                                "input": serde_json::from_str::<serde_json::Value>(&tc.function.arguments).unwrap_or_default()
                            })),
                            AssistantContent::Reasoning(_) => None,
                        })
                        .collect();
                    api_messages.push(AnthropicMessage {
                        role: "assistant".to_string(),
                        content: serde_json::Value::Array(blocks),
                    });
                }
                ChatMessage::Tool {
                    tool_call_id,
                    content,
                } => {
                    api_messages.push(AnthropicMessage {
                        role: "user".to_string(),
                        content: serde_json::json!([{
                            "type": "tool_result",
                            "tool_use_id": tool_call_id,
                            "content": content
                        }]),
                    });
                }
            }
        }

        let api_tools = tools.map(|ts| {
            ts.iter()
                .map(|t| AnthropicTool {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    input_schema: t.parameters.clone(),
                })
                .collect()
        });

        let api_tool_choice = tool_choice.map(|tc| match tc {
            ToolChoice::_Auto => AnthropicToolChoice {
                choice_type: "auto".to_string(),
                name: None,
            },
            ToolChoice::Required => AnthropicToolChoice {
                choice_type: "any".to_string(),
                name: None,
            },
            ToolChoice::_None => AnthropicToolChoice {
                choice_type: "none".to_string(),
                name: None,
            },
        });

        let request = AnthropicRequest {
            model: model.to_string(),
            system: system_text,
            messages: api_messages,
            max_tokens: max_tokens as u64,
            temperature,
            tools: api_tools,
            tool_choice: api_tool_choice,
        };

        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));

        let resp = self
            .http
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .context("Failed to send Anthropic request")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "Anthropic API error ({}): {}",
                status,
                body.chars().take(500).collect::<String>()
            );
        }

        let api_resp: AnthropicResponse = resp
            .json()
            .await
            .context("Failed to parse Anthropic response")?;

        Ok(anthropic_to_completion(api_resp))
    }

    // ── Gemini path ────────────────────────────────────────────────────

    async fn send_gemini(
        &self,
        model: &str,
        messages: &[ChatMessage],
        max_tokens: u32,
        temperature: Option<f64>,
        tools: Option<&[ToolDefinition]>,
        _tool_choice: Option<&ToolChoice>,
    ) -> Result<CompletionResponse> {
        let mut system_instruction = None;
        let mut contents = Vec::new();

        for msg in messages {
            match msg {
                ChatMessage::System { content } => {
                    system_instruction = Some(GeminiContent {
                        role: None,
                        parts: vec![GeminiPart::Text {
                            text: content.clone(),
                        }],
                    });
                }
                ChatMessage::User { content } => {
                    contents.push(GeminiContent {
                        role: Some("user".to_string()),
                        parts: vec![GeminiPart::Text {
                            text: content.clone(),
                        }],
                    });
                }
                ChatMessage::Assistant { content } => {
                    let parts: Vec<GeminiPart> = content
                        .iter()
                        .filter_map(|c| match c {
                            AssistantContent::Text(t) => Some(GeminiPart::Text {
                                text: t.text.clone(),
                            }),
                            AssistantContent::ToolCall(tc) => {
                                let args = serde_json::from_str(&tc.function.arguments)
                                    .unwrap_or_default();
                                Some(GeminiPart::FunctionCall {
                                    function_call: GeminiFunctionCall {
                                        name: tc.function.name.clone(),
                                        args,
                                    },
                                })
                            }
                            AssistantContent::Reasoning(_) => None,
                        })
                        .collect();
                    contents.push(GeminiContent {
                        role: Some("model".to_string()),
                        parts,
                    });
                }
                ChatMessage::Tool {
                    tool_call_id: _,
                    content,
                } => {
                    // Gemini uses functionResponse in model turn
                    let response_value = serde_json::from_str(content)
                        .unwrap_or(serde_json::json!({"result": content}));
                    contents.push(GeminiContent {
                        role: Some("function".to_string()),
                        parts: vec![GeminiPart::FunctionResponse {
                            function_response: GeminiFunctionResponse {
                                name: "tool_result".to_string(),
                                response: response_value,
                            },
                        }],
                    });
                }
            }
        }

        let api_tools = tools.map(|ts| {
            vec![GeminiToolDeclaration {
                function_declarations: ts
                    .iter()
                    .map(|t| GeminiFunctionDecl {
                        name: t.name.clone(),
                        description: t.description.clone(),
                        parameters: t.parameters.clone(),
                    })
                    .collect(),
            }]
        });

        let request = GeminiRequest {
            contents,
            system_instruction,
            generation_config: Some(GeminiGenerationConfig {
                max_output_tokens: Some(max_tokens as u64),
                temperature,
            }),
            tools: api_tools,
        };

        let url = format!(
            "{}/v1beta/models/{}:generateContent?key={}",
            self.base_url.trim_end_matches('/'),
            model,
            self.api_key
        );

        let resp = self
            .http
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .context("Failed to send Gemini request")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "Gemini API error ({}): {}",
                status,
                body.chars().take(500).collect::<String>()
            );
        }

        let api_resp: GeminiResponse = resp
            .json()
            .await
            .context("Failed to parse Gemini response")?;

        let candidate = api_resp
            .candidates
            .into_iter()
            .next()
            .context("Empty candidates in Gemini response")?;

        Ok(gemini_to_completion(candidate.content))
    }
}

// ── ProviderAgent ──────────────────────────────────────────────────────────

/// Unified LLM agent that supports single-turn and multi-turn dialogue.
pub struct ProviderAgent {
    client: ProviderClient,
    model: String,
    system_prompt: String,
    max_tokens: u32,
    temperature: Option<f64>,
    tools: Vec<Arc<dyn AgentTool>>,
    tool_choice: Option<ToolChoice>,
}

impl ProviderAgent {
    /// Execute a single-turn prompt and return the text response.
    pub async fn prompt(&self, user_prompt: &str) -> Result<String> {
        // CodexRs uses subprocess
        if let Some(ref codex) = self.client.codex_client
            && self.client.provider == LLMProvider::CodexRs
        {
            return codex
                .prompt_with_model(&self.system_prompt, user_prompt, Some(&self.model))
                .await;
        }

        let messages = vec![
            ChatMessage::system(&self.system_prompt),
            ChatMessage::user(user_prompt),
        ];

        let resp = self
            .client
            .send_completion(
                &self.model,
                &messages,
                self.max_tokens,
                self.temperature,
                None,
                None,
            )
            .await?;

        Ok(resp.text_content())
    }

    /// Execute multi-turn dialogue with tool calling.
    pub async fn multi_turn(
        &self,
        user_prompt: &str,
        max_iterations: usize,
    ) -> Result<String, PromptError> {
        // CodexRs fallback — no multi-turn, delegate to single prompt
        if let Some(ref codex) = self.client.codex_client
            && self.client.provider == LLMProvider::CodexRs
        {
            return codex
                .prompt_with_model(&self.system_prompt, user_prompt, Some(&self.model))
                .await
                .map_err(PromptError::CompletionError);
        }

        let tool_defs: Vec<ToolDefinition> = self.tools.iter().map(|t| t.definition()).collect();
        let has_tools = !tool_defs.is_empty();

        let mut messages = vec![
            ChatMessage::system(&self.system_prompt),
            ChatMessage::user(user_prompt),
        ];

        for _iteration in 0..max_iterations {
            let resp = self
                .client
                .send_completion(
                    &self.model,
                    &messages,
                    self.max_tokens,
                    self.temperature,
                    if has_tools { Some(&tool_defs) } else { None },
                    self.tool_choice.as_ref(),
                )
                .await
                .map_err(PromptError::CompletionError)?;

            if resp.tool_calls.is_empty() {
                // No tool calls — return text response
                return Ok(resp.text_content());
            }

            // Build assistant message with tool calls (and any reasoning traces).
            let mut assistant_content = Vec::new();
            if !resp.reasoning.is_empty() {
                assistant_content.push(AssistantContent::Reasoning(ReasoningContent {
                    reasoning: resp.reasoning.clone(),
                }));
            }
            if !resp.text.is_empty() {
                assistant_content.push(AssistantContent::Text(TextContent {
                    text: resp.text.clone(),
                }));
            }
            for tc in &resp.tool_calls {
                assistant_content.push(AssistantContent::ToolCall(tc.clone()));
            }
            messages.push(ChatMessage::Assistant {
                content: assistant_content,
            });

            // Execute tool calls
            for tc in &resp.tool_calls {
                let result = self.execute_tool_call(tc).await;
                messages.push(ChatMessage::tool_result(&tc.id, &result));
            }
        }

        // Max iterations reached
        Err(PromptError::MaxDepthError {
            max_depth: max_iterations,
            chat_history: messages,
            _prompt: user_prompt.to_string(),
        })
    }

    /// Execute a single tool call by dispatching to the matching registered tool.
    async fn execute_tool_call(&self, tool_call: &ToolCallInfo) -> String {
        for tool in &self.tools {
            if tool.name() == tool_call.function.name {
                match tool.call_json(&tool_call.function.arguments).await {
                    Ok(result) => return result,
                    Err(e) => {
                        return format!("{{\"error\": \"{}\"}}", e);
                    }
                }
            }
        }
        format!(
            "{{\"error\": \"Unknown tool: {}\"}}",
            tool_call.function.name
        )
    }
}

// ── ProviderExtractor ──────────────────────────────────────────────────────

/// Unified extractor for structured output from LLMs.
pub enum ProviderExtractor<T>
where
    T: JsonSchema + for<'a> Deserialize<'a> + Serialize + Send + Sync + 'static,
{
    /// Function-calling extraction (OpenAI, Anthropic, Gemini, etc.)
    FunctionCalling {
        client: ProviderClient,
        model: String,
        system_prompt: String,
        max_tokens: u32,
        _phantom: std::marker::PhantomData<T>,
    },
    /// Ollama prompt-based extraction with JSON parsing cascade.
    Ollama(OllamaExtractorWrapper<T>),
    /// CodexRs subprocess extraction.
    CodexRs {
        client: CodexRsClient,
        model: String,
        system_prompt: String,
        _phantom: std::marker::PhantomData<T>,
    },
}

impl<T> ProviderExtractor<T>
where
    T: JsonSchema + for<'a> Deserialize<'a> + Serialize + Send + Sync + 'static,
{
    /// Execute extraction.
    pub async fn extract(&self, prompt: &str) -> Result<T> {
        match self {
            ProviderExtractor::FunctionCalling {
                client,
                model,
                system_prompt,
                max_tokens,
                ..
            } => {
                Self::extract_via_function_calling(
                    client,
                    model,
                    system_prompt,
                    *max_tokens,
                    prompt,
                )
                .await
            }
            ProviderExtractor::Ollama(wrapper) => wrapper.extract(prompt).await,
            ProviderExtractor::CodexRs {
                client,
                model,
                system_prompt,
                ..
            } => {
                client
                    .extract_with_model::<T>(system_prompt, prompt, Some(model))
                    .await
            }
        }
    }

    /// Extract structured data using function calling.
    ///
    /// Sends a request with a single tool whose parameters match the JSON Schema
    /// of `T`, forces the model to call that tool, and parses the arguments as `T`.
    async fn extract_via_function_calling(
        client: &ProviderClient,
        model: &str,
        system_prompt: &str,
        max_tokens: u32,
        user_prompt: &str,
    ) -> Result<T> {
        let schema = schemars::schema_for!(T);
        let schema_json = serde_json::to_value(&schema)?;

        let extract_tool = ToolDefinition {
            name: "extract_data".to_string(),
            description: "Extract structured data from the provided text.".to_string(),
            parameters: schema_json,
        };

        let messages = vec![
            ChatMessage::system(system_prompt),
            ChatMessage::user(user_prompt),
        ];

        // Force the model to call our extraction tool
        let forced_choice = ToolChoice::Required;
        let resp = client
            .send_completion(
                model,
                &messages,
                max_tokens,
                None,
                Some(&[extract_tool]),
                Some(&forced_choice),
            )
            .await?;

        // Find the tool call with our extraction function
        for tc in &resp.tool_calls {
            if tc.function.name == "extract_data" {
                let result: T =
                    serde_json::from_str(&tc.function.arguments).with_context(|| {
                        format!(
                            "Failed to deserialize extracted data: {}",
                            tc.function.arguments.chars().take(200).collect::<String>()
                        )
                    })?;
                return Ok(result);
            }
        }

        // If no tool call was found, try parsing the text response as JSON
        if !resp.text.is_empty()
            && let Ok(result) = serde_json::from_str::<T>(&resp.text)
        {
            return Ok(result);
        }

        anyhow::bail!("Model did not produce structured extraction output")
    }
}

// ── Internal completion response ───────────────────────────────────────────

/// Normalized completion response (provider-agnostic).
struct CompletionResponse {
    text: String,
    tool_calls: Vec<ToolCallInfo>,
    /// Reasoning traces from models that expose chain-of-thought (e.g. Anthropic thinking blocks).
    reasoning: Vec<String>,
}

impl CompletionResponse {
    fn text_content(&self) -> String {
        self.text.clone()
    }
}

// ── Conversion helpers ─────────────────────────────────────────────────────

fn chat_to_openai(msg: &ChatMessage) -> OpenAIMessage {
    match msg {
        ChatMessage::System { content } => OpenAIMessage {
            role: "system".to_string(),
            content: Some(content.clone()),
            tool_calls: None,
            tool_call_id: None,
        },
        ChatMessage::User { content } => OpenAIMessage {
            role: "user".to_string(),
            content: Some(content.clone()),
            tool_calls: None,
            tool_call_id: None,
        },
        ChatMessage::Assistant { content } => {
            let mut text_parts = Vec::new();
            let mut tool_calls = Vec::new();

            for c in content {
                match c {
                    AssistantContent::Text(t) => text_parts.push(t.text.clone()),
                    AssistantContent::ToolCall(tc) => {
                        tool_calls.push(OpenAIToolCall {
                            id: tc.id.clone(),
                            call_type: "function".to_string(),
                            function: OpenAIFunctionCall {
                                name: tc.function.name.clone(),
                                arguments: tc.function.arguments.clone(),
                            },
                        });
                    }
                    AssistantContent::Reasoning(_) => {}
                }
            }

            OpenAIMessage {
                role: "assistant".to_string(),
                content: if text_parts.is_empty() {
                    None
                } else {
                    Some(text_parts.join("\n"))
                },
                tool_calls: if tool_calls.is_empty() {
                    None
                } else {
                    Some(tool_calls)
                },
                tool_call_id: None,
            }
        }
        ChatMessage::Tool {
            tool_call_id,
            content,
        } => OpenAIMessage {
            role: "tool".to_string(),
            content: Some(content.clone()),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.clone()),
        },
    }
}

fn openai_to_completion(msg: OpenAIMessage) -> CompletionResponse {
    let text = msg.content.unwrap_or_default();
    let tool_calls = msg
        .tool_calls
        .unwrap_or_default()
        .into_iter()
        .map(|tc| ToolCallInfo {
            id: tc.id,
            function: FunctionCall {
                name: tc.function.name,
                arguments: tc.function.arguments,
            },
        })
        .collect();

    CompletionResponse {
        text,
        tool_calls,
        reasoning: Vec::new(),
    }
}

fn anthropic_to_completion(resp: AnthropicResponse) -> CompletionResponse {
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();
    let mut reasoning = Vec::new();

    for block in resp.content {
        match block {
            AnthropicContentBlock::Text { text } => text_parts.push(text),
            AnthropicContentBlock::ToolUse { id, name, input } => {
                tool_calls.push(ToolCallInfo {
                    id,
                    function: FunctionCall {
                        name,
                        arguments: serde_json::to_string(&input).unwrap_or_default(),
                    },
                });
            }
            // Collect extended thinking traces so callers can surface them via AssistantContent::Reasoning.
            AnthropicContentBlock::Thinking { thinking } => reasoning.push(thinking),
        }
    }

    CompletionResponse {
        text: text_parts.join("\n"),
        tool_calls,
        reasoning,
    }
}

fn gemini_to_completion(content: GeminiContent) -> CompletionResponse {
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();

    for part in content.parts {
        match part {
            GeminiPart::Text { text } => text_parts.push(text),
            GeminiPart::FunctionCall { function_call } => {
                tool_calls.push(ToolCallInfo {
                    id: format!("gemini_{}", uuid::Uuid::new_v4()),
                    function: FunctionCall {
                        name: function_call.name,
                        arguments: serde_json::to_string(&function_call.args).unwrap_or_default(),
                    },
                });
            }
            GeminiPart::FunctionResponse { .. } => {}
        }
    }

    CompletionResponse {
        text: text_parts.join("\n"),
        tool_calls,
        reasoning: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    struct TestExtraction {
        value: String,
    }

    #[test]
    fn create_extractor_codexrs_keeps_requested_model() {
        let cfg = LLMConfig {
            provider: LLMProvider::CodexRs,
            codex_binary_path: Some("/fake/codex".to_string()),
            ..LLMConfig::default()
        };

        let client = ProviderClient::new(&cfg).unwrap();
        let extractor = client.create_extractor::<TestExtraction>("o3-mini", "sys", &cfg);

        match extractor {
            ProviderExtractor::CodexRs {
                model,
                system_prompt,
                ..
            } => {
                assert_eq!(model, "o3-mini");
                assert_eq!(system_prompt, "sys");
            }
            _ => panic!("expected CodexRs extractor"),
        }
    }

    #[test]
    fn create_extractor_function_calling_keeps_requested_model() {
        let cfg = LLMConfig {
            provider: LLMProvider::OpenAI,
            ..LLMConfig::default()
        };
        let client = ProviderClient::new(&cfg).unwrap();
        let extractor = client.create_extractor::<TestExtraction>("deepseek-chat", "sys", &cfg);

        match extractor {
            ProviderExtractor::FunctionCalling {
                model,
                system_prompt,
                ..
            } => {
                assert_eq!(model, "deepseek-chat");
                assert_eq!(system_prompt, "sys");
            }
            _ => panic!("expected function-calling extractor"),
        }
    }
}
