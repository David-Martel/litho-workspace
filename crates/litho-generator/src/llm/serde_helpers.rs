//! Serde helpers for LLM output deserialization hardening.
//!
//! These are `pub` API helpers intended for use via `#[serde(deserialize_with)]`
//! in struct definitions. The compiler cannot see those usages through the macro
//! expansion, so the module suppresses the resulting false-positive dead_code lint.
//!
//! Local 7B–8B LLMs (e.g. `qwen2.5-coder:7b`) frequently produce JSON that
//! deviates from the requested schema. These helpers implement tolerant
//! deserialization strategies ported from the deepwiki-rs hardening patches
//! (Patches 3, 5, 6, 8).
// Inner attribute: suppress dead_code for public deserializer helpers that are
// only referenced through #[serde(deserialize_with = "...")] macro expansions.
#![allow(dead_code)]
//!
//! # Design
//!
//! Each helper is a free function suitable for use with
//! `#[serde(deserialize_with = "...")]`. They consume the raw [`serde_json::Value`]
//! first and then apply type coercion, which avoids leaking format-specific
//! details into the caller.
//!
//! # Example
//!
//! ```rust
//! use serde::Deserialize;
//! use litho_generator::llm::serde_helpers;
//!
//! #[derive(Deserialize)]
//! struct MyOutput {
//!     #[serde(deserialize_with = "serde_helpers::string_or_vec")]
//!     tags: Vec<String>,
//!
//!     #[serde(deserialize_with = "serde_helpers::number_or_string")]
//!     version: String,
//! }
//! ```

use serde::{Deserialize, Deserializer, de::DeserializeOwned};

// ────────────────────────────────────────────────────────────────────────────
// Public deserializer helpers
// ────────────────────────────────────────────────────────────────────────────

/// Deserialize a `Vec<String>` that also accepts a bare `String`.
///
/// LLMs sometimes return a single string where an array is expected. This
/// helper wraps it in a single-element vector. A `null` produces an empty
/// vector. Each element of an existing array is converted via
/// [`value_to_string`].
///
/// # Errors
///
/// Returns a [`serde::de::Error`] only if the underlying JSON value cannot be
/// obtained at all, which in practice never happens with `serde_json`.
pub fn string_or_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let val = serde_json::Value::deserialize(deserializer)?;
    Ok(match val {
        serde_json::Value::Null => Vec::new(),
        serde_json::Value::Array(arr) => arr.into_iter().map(value_to_string).collect(),
        other => vec![value_to_string(other)],
    })
}

/// Deserialize a value where a `T: Default + DeserializeOwned` is expected,
/// but the LLM may have returned a JSON-encoded string containing the object,
/// a `null`, or an unexpected primitive.
///
/// Strategy:
/// 1. If the value matches `T` directly, use it.
/// 2. If it is a `String`, attempt to parse it as JSON and then as `T`.
/// 3. Fall back to `T::default()`.
///
/// This never returns `Err`; failures silently fall back to the default.
pub fn string_or_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + DeserializeOwned,
{
    let val = serde_json::Value::deserialize(deserializer)?;

    // Fast path: direct deserialization.
    if let Ok(typed) = serde_json::from_value::<T>(val.clone()) {
        return Ok(typed);
    }

    // If the LLM double-encoded the value as a JSON string, try to decode it.
    if let serde_json::Value::String(s) = &val
        && let Ok(inner) = serde_json::from_str::<T>(s)
    {
        return Ok(inner);
    }

    Ok(T::default())
}

/// Deserialize a `String` that may arrive as a JSON number, boolean, or
/// `null`.
///
/// Mapping:
/// - `String` → kept as-is
/// - `Number` → converted via `.to_string()`
/// - `Bool`   → `"true"` / `"false"`
/// - `Null`   → `""` (empty string)
/// - `Array`  → elements joined with `", "`
/// - `Object` → tries `name` key first, then falls back to JSON
pub fn number_or_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let val = serde_json::Value::deserialize(deserializer)?;
    Ok(value_to_string(val))
}

/// Deserialize an `Option<String>` that may arrive as a number, boolean,
/// array, or object. A `null` or empty string becomes `None`.
pub fn optional_number_or_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let val = serde_json::Value::deserialize(deserializer)?;
    Ok(match val {
        serde_json::Value::Null => None,
        serde_json::Value::String(s) if s.is_empty() => None,
        serde_json::Value::Array(ref arr) if arr.is_empty() => None,
        other => {
            let s = value_to_string(other);
            if s.is_empty() { None } else { Some(s) }
        }
    })
}

/// Deserialize a `String` from any scalar or compound JSON value.
///
/// This is the canonical coercion used by all other helpers in this module.
///
/// - `String` → returned unchanged
/// - `Number` → `.to_string()` (e.g. `42` → `"42"`, `3.14` → `"3.14"`)
/// - `Bool` → `"true"` or `"false"`
/// - `Null` → `""` (empty string)
/// - `Array` → elements joined with `", "` (non-string elements are
///   coerced recursively); objects inside the array prefer their `name` field
/// - `Object` → the value of the `name` field when present, otherwise the
///   compact JSON representation of the object
pub fn deserialize_string_flexible<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let val = serde_json::Value::deserialize(deserializer)?;
    Ok(value_to_string(val))
}

/// Deserialize an `Option<String>` with full type flexibility.
///
/// Returns `None` for `null`, empty strings, and empty arrays.
pub fn deserialize_optional_string_flexible<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    optional_number_or_string(deserializer)
}

// ────────────────────────────────────────────────────────────────────────────
// Internal helpers
// ────────────────────────────────────────────────────────────────────────────

/// Convert any [`serde_json::Value`] to its best-effort string representation.
///
/// This is the single source of truth for all string-coercion logic in this
/// module.
pub(crate) fn value_to_string(val: serde_json::Value) -> String {
    match val {
        serde_json::Value::String(s) => s,
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => String::new(),
        serde_json::Value::Array(arr) => {
            let parts: Vec<String> = arr
                .into_iter()
                .map(|v| match v {
                    serde_json::Value::Object(ref map) => map
                        .get("name")
                        .and_then(|n| n.as_str())
                        .map(String::from)
                        .unwrap_or_else(|| v.to_string()),
                    other => value_to_string(other),
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
    }
}

/// Unwrap common LLM response envelope wrappers.
///
/// Some models wrap their structured answer in an extra object layer.
/// This function strips the following patterns (deepwiki-rs Patch 3):
///
/// - `{"result": {...}}` → `{...}`
/// - `{"data": {...}}`   → `{...}`
/// - `{"output": {...}}` → `{...}`
/// - Double-encoded JSON string: `{"result": "{...}"}` → tries to parse
///   the string value as JSON
///
/// If none of the patterns match the original value is returned unchanged.
pub(crate) fn unwrap_envelope(val: serde_json::Value) -> serde_json::Value {
    let obj = match val.as_object() {
        Some(o) => o,
        None => return val,
    };

    for key in &["result", "data", "output"] {
        if let Some(inner) = obj.get(*key) {
            match inner {
                // Inner value is already an object — unwrap directly.
                serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                    return inner.clone();
                }
                // Inner value is a string — try double-encoded JSON.
                serde_json::Value::String(s) => {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(s)
                        && (parsed.is_object() || parsed.is_array())
                    {
                        return parsed;
                    }
                }
                _ => {}
            }
        }
    }

    val
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use serde_json::json;

    // ── string_or_vec ────────────────────────────────────────────────────────

    #[derive(Deserialize, Debug, PartialEq)]
    struct WithVec {
        #[serde(deserialize_with = "string_or_vec")]
        items: Vec<String>,
    }

    #[test]
    fn string_or_vec_from_array() {
        let v: WithVec = serde_json::from_value(json!({"items": ["a", "b"]})).unwrap();
        assert_eq!(v.items, vec!["a", "b"]);
    }

    #[test]
    fn string_or_vec_from_bare_string() {
        let v: WithVec = serde_json::from_value(json!({"items": "hello"})).unwrap();
        assert_eq!(v.items, vec!["hello"]);
    }

    #[test]
    fn string_or_vec_from_null() {
        let v: WithVec = serde_json::from_value(json!({"items": null})).unwrap();
        assert_eq!(v.items, Vec::<String>::new());
    }

    #[test]
    fn string_or_vec_from_number() {
        let v: WithVec = serde_json::from_value(json!({"items": 42})).unwrap();
        assert_eq!(v.items, vec!["42"]);
    }

    #[test]
    fn string_or_vec_from_named_objects() {
        // LLMs sometimes return [{name: "foo"}, {name: "bar"}] for Vec<String>
        let v: WithVec =
            serde_json::from_value(json!({"items": [{"name": "foo"}, {"name": "bar"}]})).unwrap();
        assert_eq!(v.items, vec!["foo", "bar"]);
    }

    // ── number_or_string ────────────────────────────────────────────────────

    #[derive(Deserialize, Debug, PartialEq)]
    struct WithString {
        #[serde(deserialize_with = "number_or_string")]
        val: String,
    }

    #[test]
    fn number_or_string_integer() {
        let v: WithString = serde_json::from_value(json!({"val": 7})).unwrap();
        assert_eq!(v.val, "7");
    }

    #[test]
    fn number_or_string_float() {
        let v: WithString = serde_json::from_value(json!({"val": 2.78})).unwrap();
        assert_eq!(v.val, "2.78");
    }

    #[test]
    fn number_or_string_bool_true() {
        let v: WithString = serde_json::from_value(json!({"val": true})).unwrap();
        assert_eq!(v.val, "true");
    }

    #[test]
    fn number_or_string_null_becomes_empty() {
        let v: WithString = serde_json::from_value(json!({"val": null})).unwrap();
        assert_eq!(v.val, "");
    }

    #[test]
    fn number_or_string_passthrough() {
        let v: WithString = serde_json::from_value(json!({"val": "hello"})).unwrap();
        assert_eq!(v.val, "hello");
    }

    #[test]
    fn number_or_string_array_joined() {
        let v: WithString = serde_json::from_value(json!({"val": ["x", "y"]})).unwrap();
        assert_eq!(v.val, "x, y");
    }

    // ── optional_number_or_string ────────────────────────────────────────────

    #[derive(Deserialize, Debug, PartialEq)]
    struct WithOptString {
        #[serde(deserialize_with = "optional_number_or_string")]
        val: Option<String>,
    }

    #[test]
    fn optional_null_is_none() {
        let v: WithOptString = serde_json::from_value(json!({"val": null})).unwrap();
        assert_eq!(v.val, None);
    }

    #[test]
    fn optional_empty_string_is_none() {
        let v: WithOptString = serde_json::from_value(json!({"val": ""})).unwrap();
        assert_eq!(v.val, None);
    }

    #[test]
    fn optional_empty_array_is_none() {
        let v: WithOptString = serde_json::from_value(json!({"val": []})).unwrap();
        assert_eq!(v.val, None);
    }

    #[test]
    fn optional_number_becomes_some() {
        let v: WithOptString = serde_json::from_value(json!({"val": 99})).unwrap();
        assert_eq!(v.val, Some("99".to_string()));
    }

    #[test]
    fn optional_string_passthrough() {
        let v: WithOptString = serde_json::from_value(json!({"val": "yes"})).unwrap();
        assert_eq!(v.val, Some("yes".to_string()));
    }

    // ── string_or_default ────────────────────────────────────────────────────

    #[derive(Deserialize, Debug, PartialEq, Default)]
    struct Inner {
        #[serde(default)]
        x: i32,
    }

    #[derive(Deserialize, Debug, PartialEq)]
    struct WithDefault {
        #[serde(deserialize_with = "string_or_default")]
        inner: Inner,
    }

    #[test]
    fn string_or_default_direct_object() {
        let v: WithDefault = serde_json::from_value(json!({"inner": {"x": 5}})).unwrap();
        assert_eq!(v.inner, Inner { x: 5 });
    }

    #[test]
    fn string_or_default_double_encoded() {
        // LLM returned the object as a JSON-encoded string
        let v: WithDefault = serde_json::from_value(json!({"inner": "{\"x\": 7}"})).unwrap();
        assert_eq!(v.inner, Inner { x: 7 });
    }

    #[test]
    fn string_or_default_fallback_on_garbage() {
        let v: WithDefault = serde_json::from_value(json!({"inner": "not json"})).unwrap();
        assert_eq!(v.inner, Inner::default());
    }

    #[test]
    fn string_or_default_fallback_on_null() {
        let v: WithDefault = serde_json::from_value(json!({"inner": null})).unwrap();
        assert_eq!(v.inner, Inner::default());
    }

    // ── unwrap_envelope ──────────────────────────────────────────────────────

    #[test]
    fn unwrap_result_key() {
        let val = json!({"result": {"foo": 1}});
        let unwrapped = unwrap_envelope(val);
        assert_eq!(unwrapped, json!({"foo": 1}));
    }

    #[test]
    fn unwrap_data_key() {
        let val = json!({"data": {"bar": 2}});
        let unwrapped = unwrap_envelope(val);
        assert_eq!(unwrapped, json!({"bar": 2}));
    }

    #[test]
    fn unwrap_output_key() {
        let val = json!({"output": {"baz": 3}});
        let unwrapped = unwrap_envelope(val);
        assert_eq!(unwrapped, json!({"baz": 3}));
    }

    #[test]
    fn unwrap_double_encoded_json_string() {
        // Some models encode the JSON as a string inside the result key
        let inner = json!({"x": 42}).to_string();
        let val = json!({"result": inner});
        let unwrapped = unwrap_envelope(val);
        assert_eq!(unwrapped, json!({"x": 42}));
    }

    #[test]
    fn unwrap_no_match_passthrough() {
        let val = json!({"name": "foo", "value": 1});
        let unwrapped = unwrap_envelope(val.clone());
        assert_eq!(unwrapped, val);
    }

    #[test]
    fn unwrap_non_object_passthrough() {
        let val = json!([1, 2, 3]);
        let unwrapped = unwrap_envelope(val.clone());
        assert_eq!(unwrapped, val);
    }

    // ── value_to_string ──────────────────────────────────────────────────────

    #[test]
    fn value_to_string_named_object_in_array() {
        // The array branch extracts the `name` key from objects and joins all
        // elements with ", ".
        let arr = json!([{"name": "alpha"}, {"name": "beta"}, "gamma"]);
        let result = value_to_string(arr);
        assert_eq!(result, "alpha, beta, gamma");
    }

    #[test]
    fn value_to_string_object_without_name_key() {
        // Objects without a `name` key fall back to compact JSON.
        let val = json!({"key": "val"});
        let result = value_to_string(val);
        // The exact JSON serialization is deterministic for a single-key object.
        assert_eq!(result, "{\"key\":\"val\"}");
    }

    #[test]
    fn value_to_string_object_with_name_key() {
        let val = json!({"name": "my-name", "other": 42});
        let result = value_to_string(val);
        assert_eq!(result, "my-name");
    }
}
