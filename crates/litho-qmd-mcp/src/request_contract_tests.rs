use super::{
    PROMPT_GET_PARAMS_SCHEMA, RESOURCE_READ_PARAMS_SCHEMA, TOOLS_CALL_PARAMS_SCHEMA,
    parse_tool_call_params, validate_object_schema,
};
use serde_json::json;

// Protects: optional MCP request metadata on all three strict parameter schemas.
// Detects: rejecting a client progress token before dispatching a valid request.
// Needs: no database, network, model or device.
// Breadcrumb: crates/litho-qmd-mcp/src/main.rs (request parameter schemas).
#[test]
fn protocol_metadata_is_accepted_without_becoming_a_tool_argument() {
    let parsed = parse_tool_call_params(&json!({
        "name": "status",
        "arguments": {},
        "_meta": { "progressToken": "qmd-status" }
    }))
    .expect("protocol metadata should be accepted");
    assert_eq!(parsed.0, "status");
    assert_eq!(parsed.1, json!({}));

    for (params, schema, path) in [
        (
            json!({"uri": "qmd://notes/example.md", "_meta": {"progressToken": 7}}),
            RESOURCE_READ_PARAMS_SCHEMA,
            "resources/read.params",
        ),
        (
            json!({"name": "search", "_meta": {"progressToken": "prompt"}}),
            PROMPT_GET_PARAMS_SCHEMA,
            "prompts/get.params",
        ),
    ] {
        validate_object_schema(&params, schema, path)
            .expect("protocol metadata should be accepted");
    }
}

// Protects: the object-only metadata boundary and strict tool argument schemas.
// Detects: accepting scalar metadata or treating reserved metadata as tool input.
// Needs: no database, network, model or device.
// Breadcrumb: crates/litho-qmd-mcp/src/main.rs (schema and tool argument validation).
#[test]
fn protocol_metadata_preserves_type_and_argument_validation() {
    for metadata in [json!(null), json!(1), json!("token"), json!([])] {
        for (params, schema, path) in [
            (
                json!({"name": "status", "_meta": metadata}),
                TOOLS_CALL_PARAMS_SCHEMA,
                "tools/call.params",
            ),
            (
                json!({"uri": "qmd://notes/example.md", "_meta": metadata}),
                RESOURCE_READ_PARAMS_SCHEMA,
                "resources/read.params",
            ),
            (
                json!({"name": "search", "_meta": metadata}),
                PROMPT_GET_PARAMS_SCHEMA,
                "prompts/get.params",
            ),
        ] {
            let err = validate_object_schema(&params, schema, path)
                .expect_err("metadata must remain an object");
            assert_eq!(err.code, -32602);
            assert!(err.message.contains("expected object"));
        }
    }

    let err = parse_tool_call_params(&json!({
        "name": "status",
        "arguments": {"_meta": {}},
        "_meta": {}
    }))
    .expect_err("protocol metadata is not a status tool argument");
    assert_eq!(err.code, -32602);
    assert!(err.message.contains("unknown field"));
}
