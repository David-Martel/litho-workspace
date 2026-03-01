use anyhow::Context as _;
use clap::Parser;
use litho_qmd_core::{DocumentRequest, MultiGetRequest, QmdService, SearchOptions};
use litho_qmd_llm::AdaptiveLlmEngine;
use litho_qmd_storage::PostgresQmdStore;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{self, BufReader};

type AppService = QmdService<PostgresQmdStore, AdaptiveLlmEngine>;

#[derive(Debug, Parser)]
#[command(
    name = "litho-qmd-mcp",
    version = env!("LITHO_BUILD_VERSION"),
    about = "Rust-native QMD MCP server"
)]
struct Cli {
    #[arg(
        long,
        value_name = "INDEX",
        help = "QMD index name, defaults to 'index'"
    )]
    index: Option<String>,

    #[arg(long, help = "Print supported tools in JSON and exit")]
    dump_capabilities: bool,

    #[arg(long, help = "Run startup checks and exit")]
    healthcheck: bool,
}

#[derive(Debug, Deserialize)]
struct RpcRequest {
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct RpcResponse {
    jsonrpc: &'static str,
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Debug, Serialize)]
struct RpcError {
    code: i64,
    message: String,
}

#[derive(Debug, Default)]
struct LifecycleState {
    shutdown_requested: bool,
    initialized_received: bool,
}

#[derive(Debug)]
enum DispatchOutcome {
    Response(RpcResponse),
    NoResponse,
    Exit(i32),
}

fn main() -> anyhow::Result<()> {
    if let Err(msg) = litho_core::build_info::assert_expected_token_sync() {
        anyhow::bail!("Build token sync check failed: {msg}");
    }

    tracing_subscriber::fmt()
        .with_env_filter("warn")
        .without_time()
        .init();

    let cli = Cli::parse();
    let store = PostgresQmdStore::open_default(cli.index.as_deref())
        .context("failed to open qmd postgres/config store")?;
    let service = QmdService::new(store, AdaptiveLlmEngine::from_env());

    if cli.healthcheck {
        let status = service.status().map_err(anyhow::Error::msg)?;
        println!(
            "ok total_documents={} has_vector_index={}",
            status.total_documents, status.has_vector_index
        );
        return Ok(());
    }

    if cli.dump_capabilities {
        println!("{}", serde_json::to_string_pretty(&capabilities_payload())?);
        return Ok(());
    }

    let exit_code = serve_stdio(service)?;
    if exit_code != 0 {
        std::process::exit(exit_code);
    }
    Ok(())
}

fn serve_stdio(service: AppService) -> anyhow::Result<i32> {
    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let mut stdout = io::stdout();
    let mut lifecycle = LifecycleState::default();

    while let Some(message) = read_message(&mut reader)? {
        let request: RpcRequest = match serde_json::from_str(&message) {
            Ok(req) => req,
            Err(err) => {
                let response = RpcResponse {
                    jsonrpc: "2.0",
                    id: None,
                    result: None,
                    error: Some(RpcError {
                        code: -32700,
                        message: format!("parse error: {err}"),
                    }),
                };
                write_message(&mut stdout, &response)?;
                continue;
            }
        };

        match dispatch_message(request, Some(&service), &mut lifecycle) {
            DispatchOutcome::Response(response) => write_message(&mut stdout, &response)?,
            DispatchOutcome::NoResponse => {}
            DispatchOutcome::Exit(exit_code) => return Ok(exit_code),
        }
    }

    Ok(0)
}

fn read_message(reader: &mut impl io::BufRead) -> anyhow::Result<Option<String>> {
    let mut first_line = String::new();
    loop {
        first_line.clear();
        let bytes = reader.read_line(&mut first_line)?;
        if bytes == 0 {
            return Ok(None);
        }
        if !first_line.trim().is_empty() {
            break;
        }
    }

    if first_line.trim_start().starts_with('{') {
        return Ok(Some(first_line));
    }

    let mut headers = BTreeMap::<String, String>::new();
    if let Some((k, v)) = parse_header(&first_line) {
        headers.insert(k, v);
    }

    loop {
        let mut header_line = String::new();
        let bytes = reader.read_line(&mut header_line)?;
        if bytes == 0 || header_line.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = parse_header(&header_line) {
            headers.insert(k, v);
        }
    }

    let length = headers
        .get("content-length")
        .and_then(|v| v.parse::<usize>().ok())
        .ok_or_else(|| anyhow::anyhow!("missing content-length header"))?;

    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload)?;
    let message = String::from_utf8(payload).context("invalid utf-8 message payload")?;
    Ok(Some(message))
}

fn parse_header(line: &str) -> Option<(String, String)> {
    let (key, value) = line.split_once(':')?;
    Some((key.trim().to_ascii_lowercase(), value.trim().to_string()))
}

fn write_message(stdout: &mut impl io::Write, response: &RpcResponse) -> anyhow::Result<()> {
    let payload = serde_json::to_vec(response)?;
    write!(stdout, "Content-Length: {}\r\n\r\n", payload.len())?;
    stdout.write_all(&payload)?;
    stdout.flush()?;
    Ok(())
}

fn dispatch_message(
    request: RpcRequest,
    service: Option<&AppService>,
    lifecycle: &mut LifecycleState,
) -> DispatchOutcome {
    let method = request.method.as_str();
    let id = request.id.clone();
    let is_notification = id.is_none();

    match method {
        "notifications/initialized" => {
            lifecycle.initialized_received = true;
            if is_notification {
                return DispatchOutcome::NoResponse;
            }
            return DispatchOutcome::Response(error_response(
                id,
                -32600,
                "notifications/initialized must be sent as a notification",
            ));
        }
        "exit" => {
            if is_notification {
                let code = if lifecycle.shutdown_requested { 0 } else { 1 };
                return DispatchOutcome::Exit(code);
            }
            return DispatchOutcome::Response(error_response(
                id,
                -32600,
                "exit must be sent as a notification",
            ));
        }
        "shutdown" => {
            lifecycle.shutdown_requested = true;
            if is_notification {
                return DispatchOutcome::NoResponse;
            }
            return DispatchOutcome::Response(success_response(id, Value::Null));
        }
        "ping" => {
            if is_notification {
                return DispatchOutcome::NoResponse;
            }
            return DispatchOutcome::Response(success_response(id, json!({})));
        }
        _ => {}
    }

    if lifecycle.shutdown_requested {
        if is_notification {
            return DispatchOutcome::NoResponse;
        }
        return DispatchOutcome::Response(error_response(
            id,
            -32600,
            "server is shutting down; only ping requests and exit notification are accepted",
        ));
    }

    if is_notification {
        return DispatchOutcome::NoResponse;
    }

    let Some(service) = service else {
        return DispatchOutcome::Response(error_response(
            id,
            -32603,
            "service unavailable for non-lifecycle request",
        ));
    };

    DispatchOutcome::Response(handle_request(request, service))
}

fn handle_request(request: RpcRequest, service: &AppService) -> RpcResponse {
    let id = request.id.clone();
    let result = match request.method.as_str() {
        "initialize" => Ok(json!({
            "protocolVersion": "2025-06-18",
            "serverInfo": { "name": "litho-qmd-mcp", "version": env!("CARGO_PKG_VERSION") },
            "capabilities": {
                "resources": { "subscribe": false, "listChanged": true },
                "tools": { "listChanged": false },
                "prompts": { "listChanged": true }
            }
        })),
        "tools/list" => Ok(capabilities_payload()),
        "tools/call" => call_tool(&request.params, service),
        "resources/list" => list_resources(service),
        "resources/read" => read_resource(&request.params, service),
        "prompts/list" => list_prompts(),
        "prompts/get" => get_prompt(&request.params),
        _ => Err(RpcError {
            code: -32601,
            message: format!("unknown method: {}", request.method),
        }),
    };

    match result {
        Ok(value) => RpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(value),
            error: None,
        },
        Err(error) => RpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(error),
        },
    }
}

fn success_response(id: Option<Value>, result: Value) -> RpcResponse {
    RpcResponse {
        jsonrpc: "2.0",
        id,
        result: Some(result),
        error: None,
    }
}

fn error_response(id: Option<Value>, code: i64, message: impl Into<String>) -> RpcResponse {
    RpcResponse {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(RpcError {
            code,
            message: message.into(),
        }),
    }
}

fn capabilities_payload() -> Value {
    json!({
        "tools": [
            {
                "name": "search",
                "description": "Fast keyword-based full-text search using BM25",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "limit": { "type": "integer", "default": 10 },
                        "minScore": { "type": "number", "default": 0.0 },
                        "collection": { "type": "string" }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "vsearch",
                "description": "Semantic search with native quantized vectors and lexical blending",
                "inputSchema": { "type": "object", "properties": { "query": { "type": "string" } }, "required": ["query"] }
            },
            {
                "name": "query",
                "description": "Hybrid query with expansion and reranking",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "limit": { "type": "integer", "default": 10 },
                        "minScore": { "type": "number", "default": 0.0 },
                        "collection": { "type": "string" }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "get",
                "description": "Retrieve a single document by path or #docid",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "file": { "type": "string" },
                        "fromLine": { "type": "integer" },
                        "maxLines": { "type": "integer" },
                        "lineNumbers": { "type": "boolean", "default": false }
                    },
                    "required": ["file"]
                }
            },
            {
                "name": "multi_get",
                "description": "Retrieve multiple documents via glob or comma-separated list",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string" },
                        "maxLines": { "type": "integer" },
                        "maxBytes": { "type": "integer", "default": 10240 },
                        "lineNumbers": { "type": "boolean", "default": false }
                    },
                    "required": ["pattern"]
                }
            },
            {
                "name": "status",
                "description": "Show index status: collections, document counts, embedding health",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "ingest",
                "description": "Scan configured collections and upsert markdown/doc files into the index",
                "inputSchema": {
                    "type": "object",
                    "properties": { "force": { "type": "boolean", "default": false } }
                }
            },
            {
                "name": "embed",
                "description": "Build native quantized semantic vectors for indexed documents",
                "inputSchema": {
                    "type": "object",
                    "properties": { "force": { "type": "boolean", "default": false } }
                }
            }
        ]
    })
}

fn call_tool(params: &Value, service: &AppService) -> Result<Value, RpcError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError {
            code: -32602,
            message: "missing tools/call.params.name".to_string(),
        })?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    match name {
        "search" => {
            let query = parse_string(&args, "query")?;
            let options = parse_search_options(&args);
            let out = service.search(&query, options).map_err(map_tool_error)?;
            Ok(json!({
                "content": [{ "type": "text", "text": format!("Found {} result(s)", out.results.len()) }],
                "structuredContent": out
            }))
        }
        "vsearch" => {
            let query = parse_string(&args, "query")?;
            let options = parse_search_options(&args);
            let out = service.vsearch(&query, options).map_err(map_tool_error)?;
            Ok(json!({
                "content": [{ "type": "text", "text": format!("Found {} result(s)", out.results.len()) }],
                "structuredContent": out
            }))
        }
        "query" => {
            let query = parse_string(&args, "query")?;
            let options = parse_search_options(&args);
            let out = service.query(&query, options).map_err(map_tool_error)?;
            Ok(json!({
                "content": [{ "type": "text", "text": format!("Found {} result(s)", out.results.len()) }],
                "structuredContent": out
            }))
        }
        "get" => {
            let file = parse_string(&args, "file")?;
            let out = service
                .get(&DocumentRequest {
                    file,
                    from_line: args
                        .get("fromLine")
                        .and_then(Value::as_u64)
                        .map(|v| v as usize),
                    max_lines: args
                        .get("maxLines")
                        .and_then(Value::as_u64)
                        .map(|v| v as usize),
                    line_numbers: args
                        .get("lineNumbers")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                })
                .map_err(map_tool_error)?;
            Ok(json!({
                "content": [{ "type": "resource", "resource": out }]
            }))
        }
        "multi_get" => {
            let pattern = parse_string(&args, "pattern")?;
            let out = service
                .multi_get(&MultiGetRequest {
                    pattern,
                    max_lines: args
                        .get("maxLines")
                        .and_then(Value::as_u64)
                        .map(|v| v as usize),
                    max_bytes: args
                        .get("maxBytes")
                        .and_then(Value::as_u64)
                        .map(|v| v as usize)
                        .unwrap_or(10 * 1024),
                    line_numbers: args
                        .get("lineNumbers")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                })
                .map_err(map_tool_error)?;
            Ok(json!({
                "content": [{ "type": "text", "text": format!("Returned {} document(s), {} warning(s)", out.documents.len(), out.errors.len()) }],
                "structuredContent": out
            }))
        }
        "status" => {
            let out = service.status().map_err(map_tool_error)?;
            Ok(json!({
                "content": [{ "type": "text", "text": format!("{} document(s)", out.total_documents) }],
                "structuredContent": out
            }))
        }
        "ingest" => {
            let force = args.get("force").and_then(Value::as_bool).unwrap_or(false);
            let out = service.ingest(force).map_err(map_tool_error)?;
            Ok(json!({
                "content": [{ "type": "text", "text": format!("indexed={} updated={} unchanged={}", out.indexed_documents, out.updated_documents, out.unchanged_documents) }],
                "structuredContent": out
            }))
        }
        "embed" => {
            let force = args.get("force").and_then(Value::as_bool).unwrap_or(false);
            let out = service.embed(force).map_err(map_tool_error)?;
            Ok(json!({
                "content": [{ "type": "text", "text": format!("embedded={} skipped={}", out.embedded, out.skipped) }],
                "structuredContent": out
            }))
        }
        _ => Err(RpcError {
            code: -32601,
            message: format!("unknown tool: {name}"),
        }),
    }
}

fn list_resources(service: &AppService) -> Result<Value, RpcError> {
    let files = service.list_files(None).map_err(map_tool_error)?;
    let resources = files
        .into_iter()
        .map(|f| {
            json!({
                "uri": format!("qmd://{}", f),
                "name": f,
                "mimeType": "text/markdown"
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({ "resources": resources }))
}

fn read_resource(params: &Value, service: &AppService) -> Result<Value, RpcError> {
    let uri = params
        .get("uri")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError {
            code: -32602,
            message: "missing resources/read.params.uri".to_string(),
        })?;
    let file = uri.strip_prefix("qmd://").unwrap_or(uri).to_string();
    let doc = service
        .get(&DocumentRequest {
            file,
            from_line: None,
            max_lines: None,
            line_numbers: false,
        })
        .map_err(map_tool_error)?;
    Ok(json!({
        "contents": [{
            "uri": doc.uri,
            "mimeType": "text/markdown",
            "text": doc.text
        }]
    }))
}

fn list_prompts() -> Result<Value, RpcError> {
    Ok(json!({
        "prompts": [{
            "name": "query",
            "title": "QMD Query Guide",
            "description": "How to search and retrieve indexed documentation with qmd"
        }]
    }))
}

fn get_prompt(params: &Value) -> Result<Value, RpcError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError {
            code: -32602,
            message: "missing prompts/get.params.name".to_string(),
        })?;
    if name != "query" {
        return Err(RpcError {
            code: -32602,
            message: format!("unknown prompt: {name}"),
        });
    }
    Ok(json!({
        "messages": [{
            "role": "user",
            "content": {
                "type": "text",
                "text": "Use tools in this order: search -> vsearch -> query -> get or multi_get. Prefer search first for exact terms; use query for best-quality fusion."
            }
        }]
    }))
}

fn parse_search_options(args: &Value) -> SearchOptions {
    SearchOptions {
        limit: args
            .get("limit")
            .and_then(Value::as_u64)
            .map(|v| v as usize)
            .unwrap_or(10),
        min_score: args
            .get("minScore")
            .and_then(Value::as_f64)
            .map(|v| v as f32)
            .unwrap_or(0.0),
        collection: args
            .get("collection")
            .and_then(Value::as_str)
            .map(str::to_string),
    }
}

fn parse_string(args: &Value, key: &str) -> Result<String, RpcError> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| RpcError {
            code: -32602,
            message: format!("missing or invalid string field '{key}'"),
        })
}

fn map_tool_error(err: impl std::fmt::Display) -> RpcError {
    RpcError {
        code: -32000,
        message: err.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(id: Option<Value>, method: &str) -> RpcRequest {
        RpcRequest {
            id,
            method: method.to_string(),
            params: Value::Null,
        }
    }

    #[test]
    fn ping_request_returns_empty_object() {
        let mut lifecycle = LifecycleState::default();
        let outcome = dispatch_message(request(Some(json!(1)), "ping"), None, &mut lifecycle);

        let DispatchOutcome::Response(response) = outcome else {
            panic!("expected response");
        };
        assert_eq!(response.id, Some(json!(1)));
        assert_eq!(response.result, Some(json!({})));
        assert!(response.error.is_none());
    }

    #[test]
    fn shutdown_request_sets_state_and_returns_null() {
        let mut lifecycle = LifecycleState::default();
        let outcome = dispatch_message(request(Some(json!(2)), "shutdown"), None, &mut lifecycle);

        assert!(lifecycle.shutdown_requested);
        let DispatchOutcome::Response(response) = outcome else {
            panic!("expected response");
        };
        assert_eq!(response.result, Some(Value::Null));
        assert!(response.error.is_none());
    }

    #[test]
    fn initialized_notification_is_acknowledged_without_response() {
        let mut lifecycle = LifecycleState::default();
        let outcome = dispatch_message(
            request(None, "notifications/initialized"),
            None,
            &mut lifecycle,
        );

        assert!(lifecycle.initialized_received);
        assert!(matches!(outcome, DispatchOutcome::NoResponse));
    }

    #[test]
    fn exit_notification_before_shutdown_returns_nonzero_exit() {
        let mut lifecycle = LifecycleState::default();
        let outcome = dispatch_message(request(None, "exit"), None, &mut lifecycle);

        assert!(matches!(outcome, DispatchOutcome::Exit(1)));
    }

    #[test]
    fn exit_notification_after_shutdown_returns_zero_exit() {
        let mut lifecycle = LifecycleState::default();
        let _ = dispatch_message(request(Some(json!(3)), "shutdown"), None, &mut lifecycle);
        let outcome = dispatch_message(request(None, "exit"), None, &mut lifecycle);

        assert!(matches!(outcome, DispatchOutcome::Exit(0)));
    }

    #[test]
    fn non_lifecycle_requests_are_rejected_after_shutdown() {
        let mut lifecycle = LifecycleState::default();
        let _ = dispatch_message(request(Some(json!(4)), "shutdown"), None, &mut lifecycle);
        let outcome = dispatch_message(request(Some(json!(5)), "tools/list"), None, &mut lifecycle);

        let DispatchOutcome::Response(response) = outcome else {
            panic!("expected response");
        };
        let error = response.error.expect("expected shutdown error");
        assert_eq!(error.code, -32600);
    }

    #[test]
    fn capabilities_payload_contains_required_tool_contracts() {
        let payload = capabilities_payload();
        let tools = payload
            .get("tools")
            .and_then(Value::as_array)
            .expect("tools array");
        let names = tools
            .iter()
            .filter_map(|t| t.get("name").and_then(Value::as_str))
            .collect::<std::collections::BTreeSet<_>>();

        for required in [
            "search",
            "vsearch",
            "query",
            "get",
            "multi_get",
            "status",
            "ingest",
            "embed",
        ] {
            assert!(
                names.contains(required),
                "missing required tool in MCP capabilities: {required}"
            );
        }
    }

    #[test]
    fn parse_search_options_defaults_and_overrides_are_stable() {
        let defaults = parse_search_options(&json!({}));
        assert_eq!(defaults.limit, 10);
        assert_eq!(defaults.min_score, 0.0);
        assert!(defaults.collection.is_none());

        let overridden = parse_search_options(&json!({
            "limit": 42,
            "minScore": 0.33,
            "collection": "docs"
        }));
        assert_eq!(overridden.limit, 42);
        assert!((overridden.min_score - 0.33).abs() < f32::EPSILON);
        assert_eq!(overridden.collection.as_deref(), Some("docs"));
    }

    #[test]
    fn parse_string_rejects_missing_or_wrong_types() {
        let err_missing = parse_string(&json!({}), "file").expect_err("missing key should fail");
        assert_eq!(err_missing.code, -32602);
        assert!(err_missing.message.contains("file"));

        let err_wrong_type =
            parse_string(&json!({ "file": 123 }), "file").expect_err("non-string should fail");
        assert_eq!(err_wrong_type.code, -32602);
    }

    #[test]
    fn build_token_matches_pipeline_when_expected_is_set() {
        if let Ok(expected) = std::env::var("LITHO_EXPECT_BUILD_TOKEN")
            && !expected.trim().is_empty()
        {
            let actual = option_env!("LITHO_BUILD_TOKEN").unwrap_or("unknown-token");
            assert_eq!(
                actual, expected,
                "compiled build token differs from LITHO_EXPECT_BUILD_TOKEN"
            );
        }
    }
}
