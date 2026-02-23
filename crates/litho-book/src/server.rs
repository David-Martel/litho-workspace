use std::sync::Arc;

use axum::{
    Router,
    extract::{Query, State},
    response::{Html, Json, Sse, sse::Event},
    routing::{get, post},
};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::time::Duration;
use tower_http::{cors::CorsLayer, services::ServeDir};
use tracing::{debug, error, info};

use crate::error::LithoBookError;
use crate::filesystem::{DocumentTree, SearchResult};
use crate::utils;

#[derive(Clone)]
pub struct AppState {
    pub doc_tree: Arc<DocumentTree>,
    pub docs_path: String,
    pub index_html: Arc<String>,
    pub http_client: reqwest::Client,
    pub llm_key: Arc<String>,
}

#[derive(Deserialize)]
pub struct FileQuery {
    file: Option<String>,
}

#[derive(Deserialize)]
pub struct SearchQuery {
    q: Option<String>,
}

#[derive(Serialize)]
pub struct FileResponse {
    pub content: String,
    pub html: String,
    pub path: String,
    pub size: Option<u64>,
    pub modified: Option<String>,
}

#[derive(Serialize)]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    pub total: usize,
    pub query: String,
}

#[derive(Serialize)]
pub struct StatsResponse {
    pub total_files: usize,
    pub total_dirs: usize,
    pub total_size: u64,
    pub formatted_size: String,
}

// AI助手相关的数据结构
#[derive(Deserialize)]
pub struct ChatRequest {
    pub message: String,
    pub context: Option<String>,             // 当前文档内容作为上下文
    pub history: Option<Vec<OpenAIMessage>>, // 历史会话消息
}

#[derive(Serialize, Deserialize)]
pub struct OpenAIMessage {
    pub role: String,
    pub content: String,
}

#[derive(Serialize, Deserialize)]
pub struct OpenAIRequest {
    pub model: String,
    pub messages: Vec<OpenAIMessage>,
    pub temperature: f32,
    pub max_tokens: i32,
    pub stream: bool,
}
// 流式响应相关的数据结构
#[derive(Deserialize)]
pub struct OpenAIStreamChoice {
    pub delta: OpenAIStreamDelta,
    pub finish_reason: Option<String>,
}

#[derive(Deserialize)]
pub struct OpenAIStreamDelta {
    pub content: Option<String>,
}

#[derive(Deserialize)]
pub struct OpenAIStreamResponse {
    pub choices: Vec<OpenAIStreamChoice>,
}

#[derive(Serialize)]
pub struct StreamEvent {
    pub event_type: String,
    pub content: Option<String>,
    pub suggestions: Option<Vec<String>>,
    pub finished: bool,
}

/// Create the main application router
pub fn create_router(doc_tree: DocumentTree, docs_path: String) -> Router {
    let tree_json = serde_json::to_string(&doc_tree.root).unwrap_or_else(|e| {
        tracing::error!("Failed to serialize document tree: {}", e);
        "{}".to_string()
    });
    let index_html = Arc::new(generate_index_html(&tree_json, &docs_path));

    let llm_key_value = std::env::var("LITHO_BOOK_LLM_KEY").unwrap_or_else(|_| {
        tracing::warn!("LITHO_BOOK_LLM_KEY not set; AI chat will not work");
        String::new()
    });

    let state = AppState {
        doc_tree: Arc::new(doc_tree),
        docs_path,
        index_html,
        http_client: reqwest::Client::new(),
        llm_key: Arc::new(llm_key_value),
    };

    Router::new()
        .route("/", get(index_handler))
        .route("/api/file", get(get_file_handler))
        .route("/api/tree", get(get_tree_handler))
        .route("/api/search", get(search_handler))
        .route("/api/stats", get(stats_handler))
        .route("/api/chat", post(chat_stream_handler))
        .route("/health", get(health_handler))
        .nest_service("/assets", ServeDir::new("assets"))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Serve the main index page
async fn index_handler(State(state): State<AppState>) -> Html<String> {
    debug!("Serving index page");
    Html((*state.index_html).clone())
}

/// Get file content and render as HTML
async fn get_file_handler(
    Query(params): Query<FileQuery>,
    State(state): State<AppState>,
) -> Result<Json<FileResponse>, LithoBookError> {
    let file_path = params.file.ok_or(LithoBookError::InvalidPath {
        path: "(missing)".to_string(),
    })?;

    debug!("Requesting file: {}", file_path);

    let content =
        state
            .doc_tree
            .get_file_content(&file_path)
            .map_err(|_| LithoBookError::FileNotFound {
                path: file_path.clone(),
            })?;

    let html = state.doc_tree.render_markdown(&content);

    // Get metadata from node_map (no disk I/O)
    let node = state.doc_tree.node_map.get(&file_path);

    let response = FileResponse {
        content,
        html,
        path: file_path,
        size: node.and_then(|n| n.size),
        modified: node.and_then(|n| n.modified.clone()),
    };

    info!("Successfully served file: {}", response.path);
    Ok(Json(response))
}

/// Get the document tree structure
async fn get_tree_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, LithoBookError> {
    debug!("Serving document tree");
    let value = serde_json::to_value(&state.doc_tree.root)?;
    Ok(Json(value))
}

/// Search for content with full-text search
async fn search_handler(
    Query(params): Query<SearchQuery>,
    State(state): State<AppState>,
) -> Json<SearchResponse> {
    let query = params.q.unwrap_or_default();

    if query.trim().is_empty() {
        return Json(SearchResponse {
            results: vec![],
            total: 0,
            query,
        });
    }

    debug!("Searching for: {}", query);

    let results = state.doc_tree.search_content(&query);
    let total = results.len();

    debug!("Found {} results matching query: {}", total, query);

    Json(SearchResponse {
        results,
        total,
        query,
    })
}

/// Get statistics about the document tree
async fn stats_handler(State(state): State<AppState>) -> Json<StatsResponse> {
    let stats = state.doc_tree.get_stats();
    Json(StatsResponse {
        total_files: stats.total_files,
        total_dirs: stats.total_dirs,
        total_size: stats.total_size,
        formatted_size: utils::format_bytes(stats.total_size),
    })
}

/// Health check endpoint
async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "version": env!("CARGO_PKG_VERSION")
    }))
}

/// AI助手流式聊天处理函数
async fn chat_stream_handler(
    State(state): State<AppState>,
    Json(request): Json<ChatRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    debug!("AI助手收到消息: {}", request.message);

    let stream = async_stream::stream! {
        match call_openai_stream_api(
            &request.message,
            request.context.as_deref(),
            request.history,
            &state.docs_path,
            &state.http_client,
            &state.llm_key,
        ).await {
            Ok(mut response_stream) => {
                let mut full_response = String::new();

                // 发送开始事件
                yield Ok(Event::default()
                    .event("start")
                    .data(serde_json::to_string(&StreamEvent {
                        event_type: "start".to_string(),
                        content: None,
                        suggestions: None,
                        finished: false,
                    }).unwrap_or_default()));

                // 处理流式响应
                while let Some(chunk) = response_stream.recv().await {
                    match chunk {
                        Ok(content) => {
                            full_response.push_str(&content);

                            // 发送内容块
                            yield Ok(Event::default()
                                .event("content")
                                .data(serde_json::to_string(&StreamEvent {
                                    event_type: "content".to_string(),
                                    content: Some(content),
                                    suggestions: None,
                                    finished: false,
                                }).unwrap_or_default()));
                        }
                        Err(e) => {
                            error!("流式响应错误: {}", e);
                            yield Ok(Event::default()
                                .event("error")
                                .data(serde_json::to_string(&StreamEvent {
                                    event_type: "error".to_string(),
                                    content: Some("抱歉，我现在无法回答您的问题。请稍后再试。".to_string()),
                                    suggestions: None,
                                    finished: true,
                                }).unwrap_or_default()));
                            return;
                        }
                    }
                }

                // 生成推荐问题
                let suggestions = generate_suggestions(&full_response, request.context.as_deref());

                // 发送完成事件
                yield Ok(Event::default()
                    .event("finish")
                    .data(serde_json::to_string(&StreamEvent {
                        event_type: "finish".to_string(),
                        content: None,
                        suggestions: Some(suggestions),
                        finished: true,
                    }).unwrap_or_default()));
            }
            Err(e) => {
                error!("调用AI API失败: {}", e);
                yield Ok(Event::default()
                    .event("error")
                    .data(serde_json::to_string(&StreamEvent {
                        event_type: "error".to_string(),
                        content: Some("抱歉，我现在无法回答您的问题。请稍后再试。".to_string()),
                        suggestions: None,
                        finished: true,
                    }).unwrap_or_default()));
            }
        }
    };

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(1))
            .text("keep-alive-text"),
    )
}

/// 调用OpenAI兼容的流式API
async fn call_openai_stream_api(
    message: &str,
    context: Option<&str>,
    history: Option<Vec<OpenAIMessage>>,
    _docs_path: &str,
    client: &reqwest::Client,
    llm_key: &str,
) -> Result<
    tokio::sync::mpsc::Receiver<Result<String, Box<dyn std::error::Error + Send + Sync>>>,
    Box<dyn std::error::Error + Send + Sync>,
> {
    // 构建系统提示词
    let mut system_prompt = "你是一个专业的文档助手，专门帮助用户理解和分析技术文档。请用中文回答问题，回答要准确、简洁、有帮助。".to_string();

    // 添加上下文（如果有的话）
    if let Some(ctx) = context
        && !ctx.is_empty()
    {
        system_prompt.push_str(&format!("\n\n用户提供的上下文信息：\n{}", ctx));
    }

    // 构建消息列表
    let mut messages = vec![OpenAIMessage {
        role: "system".to_string(),
        content: system_prompt,
    }];

    // 添加历史消息（如果有的话）
    if let Some(hist) = history {
        // 限制历史消息数量，避免请求过大
        let max_history = 10; // 最多保留10轮对话
        let start_index = if hist.len() > max_history {
            hist.len() - max_history
        } else {
            0
        };
        messages.extend(hist.into_iter().skip(start_index));
    }

    // 添加当前用户消息
    messages.push(OpenAIMessage {
        role: "user".to_string(),
        content: message.to_string(),
    });

    let request_body = OpenAIRequest {
        model: "GLM-4.7-Flash".to_string(),
        messages,
        temperature: 0.7,
        max_tokens: 16384,
        stream: true, // 启用流式响应
    };

    let response = client
        .post("https://open.bigmodel.cn/api/paas/v4/chat/completions")
        .header("Authorization", format!("Bearer {}", llm_key))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("API请求失败: {} - {}", status, text).into());
    }

    // 创建通道来传递流式数据
    let (tx, rx) = tokio::sync::mpsc::channel(100);

    // 在后台任务中处理流式响应
    tokio::spawn(async move {
        use futures::StreamExt;

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk_result) = stream.next().await {
            match chunk_result {
                Ok(chunk) => {
                    let chunk_str = String::from_utf8_lossy(&chunk);
                    buffer.push_str(&chunk_str);

                    let ends_with_newline = buffer.ends_with('\n');
                    let mut lines: Vec<String> = buffer.lines().map(str::to_string).collect();
                    let incomplete_tail = if !ends_with_newline {
                        lines.pop()
                    } else {
                        None
                    };

                    for line in &lines {
                        if let Some(data) = line.strip_prefix("data: ") {
                            if data == "[DONE]" {
                                return;
                            }

                            if let Ok(stream_response) =
                                serde_json::from_str::<OpenAIStreamResponse>(data)
                                && let Some(choice) = stream_response.choices.first()
                            {
                                if let Some(content) = &choice.delta.content
                                    && !content.is_empty()
                                    && tx.send(Ok(content.clone())).await.is_err()
                                {
                                    return;
                                }

                                if choice.finish_reason.is_some() {
                                    return;
                                }
                            }
                        }
                    }

                    buffer.clear();
                    if let Some(tail) = incomplete_tail {
                        buffer.push_str(&tail);
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(format!("流式响应错误: {}", e).into())).await;
                    return;
                }
            }
        }
    });

    Ok(rx)
}

/// 生成推荐的追问问题
fn generate_suggestions(ai_response: &str, _context: Option<&str>) -> Vec<String> {
    let mut suggestions = Vec::new();

    // 基于AI回答内容生成相关问题
    if ai_response.contains("架构") || ai_response.contains("设计") {
        suggestions.push("这个架构的优缺点是什么？".to_string());
        suggestions.push("有哪些替代的设计方案？".to_string());
    }

    if ai_response.contains("性能") || ai_response.contains("耗时") {
        suggestions.push("项目使用了哪些性能优化策略？".to_string());
        suggestions.push("如何优化项目中的性能热点？".to_string());
    }

    if ai_response.contains("配置") || ai_response.contains("参数") {
        suggestions.push("这些配置的默认值是什么？".to_string());
        suggestions.push("如何调优这些参数？".to_string());
    }

    // 如果没有特定的建议，提供通用的
    if suggestions.is_empty() {
        suggestions.push("能详细解释一下吗？".to_string());
        suggestions.push("有相关的示例吗？".to_string());
        suggestions.push("这个有什么最佳实践？".to_string());
    }

    // 限制建议数量
    suggestions.truncate(3);
    suggestions
}

/// Generate the main HTML page
fn generate_index_html(tree_json: &str, docs_path: &str) -> String {
    // Read the template file
    let template_content = include_str!("../templates/index.html.tpl");

    // Replace the placeholders with actual data
    template_content
        .replace("{{ tree_json|safe }}", tree_json)
        .replace("{{ docs_path }}", docs_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http::Request;
    use tower::ServiceExt;

    /// Build a test router backed by a temporary directory containing one
    /// `test.md` file.  The `TempDir` is returned alongside the `Router` so
    /// the caller can bind it to a local variable, keeping the directory alive
    /// for the full duration of the test.
    fn make_test_app() -> (Router, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.md"), "# Test\nHello world").unwrap();
        let tree = crate::filesystem::DocumentTree::new(dir.path()).unwrap();
        let docs_path = dir.path().display().to_string().replace('\\', "/");
        let router = create_router(tree, docs_path);
        (router, dir)
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let (app, _dir) = make_test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn test_index_endpoint() {
        let (app, _dir) = make_test_app();
        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn test_tree_endpoint() {
        let (app, _dir) = make_test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/tree")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn test_stats_endpoint() {
        let (app, _dir) = make_test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/stats")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn test_file_endpoint_found() {
        let (app, _dir) = make_test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/file?file=test.md")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn test_file_endpoint_not_found() {
        let (app, _dir) = make_test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/file?file=missing.md")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 404);
    }

    #[tokio::test]
    async fn test_file_endpoint_no_param() {
        let (app, _dir) = make_test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/file")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 400);
    }

    #[tokio::test]
    async fn test_search_endpoint() {
        let (app, _dir) = make_test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/search?q=Hello")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn test_search_endpoint_empty() {
        let (app, _dir) = make_test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/search?q=")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
    }
}
