//! Tests for the JSON recovery strategies used by the Ollama extractor.
//!
//! These tests are purely in-memory: they exercise the parsing logic that the
//! `OllamaExtractorWrapper` applies to raw LLM text output. No network calls
//! or Ollama instance is required.
//!
//! The strategies under test (mirrored here as free functions) are:
//!   1. Direct JSON parse
//!   2. Extract from markdown ```json … ``` code block
//!   3. Extract the first `{…}` object from mixed text
//!   4. Strip common markdown noise then parse

// ---------------------------------------------------------------------------
// Helper re-implementations that mirror OllamaExtractorWrapper private logic
// ---------------------------------------------------------------------------

use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

static JSON_CODE_BLOCK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"```(?:json)?\s*(\{[\s\S]*?\})\s*```").unwrap());

fn extract_from_code_block(text: &str) -> Option<String> {
    JSON_CODE_BLOCK_RE
        .captures(text)
        .and_then(|cap| cap.get(1))
        .map(|m| m.as_str().to_string())
}

fn extract_first_json_object(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let mut depth = 0usize;
    let mut end = start;
    for (i, c) in text[start..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = start + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    if depth == 0 && end > start {
        Some(text[start..end].to_string())
    } else {
        None
    }
}

fn clean_response(text: &str) -> String {
    text.trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim()
        .to_string()
}

fn unwrap_schema_wrapper(json: Value) -> Value {
    if let Some(obj) = json.as_object() {
        let has_schema = obj.contains_key("$schema") || obj.contains_key("$defs");
        let has_properties = obj.contains_key("properties");
        let has_required = obj.contains_key("required");
        let has_type_object = obj.get("type").and_then(|v| v.as_str()) == Some("object");

        if (has_schema || (has_required && has_type_object)) && has_properties {
            if let Some(properties) = obj.get("properties") {
                if properties.is_object() {
                    return properties.clone();
                }
            }
        }
    }
    json
}

/// Full recovery pipeline matching OllamaExtractorWrapper::parse_json_response
fn parse_json_response(response: &str) -> Result<Value, String> {
    // Strategy 1: direct parse
    if let Ok(v) = serde_json::from_str::<Value>(response) {
        return Ok(v);
    }
    // Strategy 2: markdown code block
    if let Some(block) = extract_from_code_block(response) {
        if let Ok(v) = serde_json::from_str::<Value>(&block) {
            return Ok(v);
        }
    }
    // Strategy 3: first JSON object
    if let Some(obj_str) = extract_first_json_object(response) {
        if let Ok(v) = serde_json::from_str::<Value>(&obj_str) {
            return Ok(v);
        }
    }
    // Strategy 4: clean & parse
    let cleaned = clean_response(response);
    serde_json::from_str::<Value>(&cleaned).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Strategy 1: valid JSON
// ---------------------------------------------------------------------------

#[test]
fn parse_valid_json_object() {
    let input = r#"{"name":"litho","version":1}"#;
    let v = parse_json_response(input).unwrap();
    assert_eq!(v["name"], "litho");
    assert_eq!(v["version"], 1);
}

#[test]
fn parse_valid_json_with_whitespace() {
    let input = "  \n  { \"key\": true }  \n  ";
    let v = parse_json_response(input).unwrap();
    assert_eq!(v["key"], true);
}

#[test]
fn parse_valid_json_nested_object() {
    let input = r#"{"outer":{"inner":"value","count":42}}"#;
    let v = parse_json_response(input).unwrap();
    assert_eq!(v["outer"]["inner"], "value");
    assert_eq!(v["outer"]["count"], 42);
}

// ---------------------------------------------------------------------------
// Strategy 2: markdown code blocks
// ---------------------------------------------------------------------------

#[test]
fn parse_json_from_markdown_json_fence() {
    let input = "Here is the result:\n```json\n{\"status\":\"ok\"}\n```\nEnd.";
    let v = parse_json_response(input).unwrap();
    assert_eq!(v["status"], "ok");
}

#[test]
fn parse_json_from_markdown_plain_fence() {
    let input = "Sure:\n```\n{\"x\":99}\n```";
    let v = parse_json_response(input).unwrap();
    assert_eq!(v["x"], 99);
}

#[test]
fn parse_json_from_markdown_multiline_object() {
    let input = r#"Result:
```json
{
  "a": 1,
  "b": "hello"
}
```
Done."#;
    let v = parse_json_response(input).unwrap();
    assert_eq!(v["a"], 1);
    assert_eq!(v["b"], "hello");
}

// ---------------------------------------------------------------------------
// Strategy 3: first JSON object in mixed text
// ---------------------------------------------------------------------------

#[test]
fn parse_json_embedded_in_prose() {
    let input = r#"The model says: {"result":"success","code":200} — end of response."#;
    let v = parse_json_response(input).unwrap();
    assert_eq!(v["result"], "success");
    assert_eq!(v["code"], 200);
}

#[test]
fn parse_json_object_after_long_preamble() {
    let input = "I will now output the JSON object as requested.\n\n{\"done\":true}";
    let v = parse_json_response(input).unwrap();
    assert_eq!(v["done"], true);
}

// ---------------------------------------------------------------------------
// Strategy 4: clean response (strip backtick noise)
// ---------------------------------------------------------------------------

#[test]
fn clean_response_strips_json_prefix() {
    let raw = "```json\n{\"k\":1}\n```";
    let cleaned = clean_response(raw);
    let v: Value = serde_json::from_str(&cleaned).unwrap();
    assert_eq!(v["k"], 1);
}

#[test]
fn clean_response_strips_plain_backtick_prefix() {
    let raw = "```\n{\"k\":2}\n```";
    let cleaned = clean_response(raw);
    let v: Value = serde_json::from_str(&cleaned).unwrap();
    assert_eq!(v["k"], 2);
}

#[test]
fn clean_response_trims_whitespace() {
    let raw = "   {\"k\":3}   ";
    let cleaned = clean_response(raw);
    assert_eq!(cleaned.trim(), "{\"k\":3}");
}

// ---------------------------------------------------------------------------
// Schema wrapper unwrapping
// ---------------------------------------------------------------------------

#[test]
fn unwrap_schema_wrapper_passthrough_plain_object() {
    let json: Value = serde_json::from_str(r#"{"name":"test","count":5}"#).unwrap();
    let result = unwrap_schema_wrapper(json.clone());
    assert_eq!(result, json);
}

#[test]
fn unwrap_schema_wrapper_extracts_properties() {
    let schema_response = serde_json::json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "type": "object",
        "required": ["name"],
        "properties": {
            "name": "litho-test",
            "score": 42
        }
    });
    let unwrapped = unwrap_schema_wrapper(schema_response);
    // Should be the properties object
    assert_eq!(unwrapped["name"], "litho-test");
    assert_eq!(unwrapped["score"], 42);
}

#[test]
fn unwrap_schema_wrapper_extracts_when_required_and_type_object() {
    let schema_response = serde_json::json!({
        "type": "object",
        "required": ["field"],
        "properties": {
            "field": "value"
        }
    });
    let unwrapped = unwrap_schema_wrapper(schema_response);
    assert_eq!(unwrapped["field"], "value");
}

#[test]
fn unwrap_schema_wrapper_no_properties_passthrough() {
    // Has $schema but no "properties" key — should NOT unwrap
    let json = serde_json::json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "data": "actual-data"
    });
    let result = unwrap_schema_wrapper(json.clone());
    // The original is returned unchanged since there are no "properties" to extract
    assert_eq!(result["data"], "actual-data");
}

// ---------------------------------------------------------------------------
// Double-encoded JSON
// ---------------------------------------------------------------------------

#[test]
fn parse_double_encoded_json_string() {
    // LLMs occasionally return JSON that was encoded into a string value
    let inner = r#"{"key":"value"}"#;
    let outer = serde_json::to_string(inner).unwrap(); // => "\"{ ... }\""
    // outer is a JSON string containing inner JSON — first parse gives a String value
    let first: Value = serde_json::from_str(&outer).unwrap();
    assert!(first.is_string(), "outer is a string value");
    // We can decode a second time
    let second: Value = serde_json::from_str(first.as_str().unwrap()).unwrap();
    assert_eq!(second["key"], "value");
}

// ---------------------------------------------------------------------------
// Error cases
// ---------------------------------------------------------------------------

#[test]
fn parse_completely_invalid_returns_err() {
    let input = "This is just plain text with no JSON whatsoever.";
    assert!(
        parse_json_response(input).is_err(),
        "expected error for non-JSON input"
    );
}

#[test]
fn parse_truncated_json_returns_err() {
    let input = r#"{"name": "litho", "version":"#;
    assert!(
        parse_json_response(input).is_err(),
        "expected error for truncated JSON"
    );
}

#[test]
fn extract_first_json_object_unmatched_braces() {
    let text = "text { unmatched";
    assert!(extract_first_json_object(text).is_none());
}

#[test]
fn extract_first_json_object_no_brace() {
    let text = "no json here";
    assert!(extract_first_json_object(text).is_none());
}

// ---------------------------------------------------------------------------
// extract_from_code_block edge cases
// ---------------------------------------------------------------------------

#[test]
fn extract_from_code_block_no_fence_returns_none() {
    let text = r#"{"key": "val"}"#; // valid JSON but no ``` fence
    assert!(extract_from_code_block(text).is_none());
}

#[test]
fn extract_from_code_block_empty_fence_returns_none() {
    let text = "```json\n```";
    // Regex requires at least `{…}` inside fence — empty block should not match
    assert!(extract_from_code_block(text).is_none());
}
