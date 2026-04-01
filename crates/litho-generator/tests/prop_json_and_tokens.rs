use litho_generator::llm::client::ollama_native::parse_json_response;
use litho_generator::utils::token_estimator::TokenEstimator;
use proptest::prelude::*;
use serde_json::{Map, Number, Value};

fn arb_json_value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(|v| Value::Number(Number::from(v))),
        ".{0,64}".prop_map(Value::String),
    ];

    leaf.prop_recursive(4, 64, 8, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..8).prop_map(Value::Array),
            prop::collection::btree_map("[a-zA-Z_][a-zA-Z0-9_]{0,12}", inner, 0..8).prop_map(|m| {
                let mut obj = Map::new();
                for (k, v) in m {
                    obj.insert(k, v);
                }
                Value::Object(obj)
            }),
        ]
    })
}

fn arb_json_object() -> impl Strategy<Value = Value> {
    prop::collection::btree_map("[a-zA-Z_][a-zA-Z0-9_]{0,12}", arb_json_value(), 0..8).prop_map(
        |m| {
            let mut obj = Map::new();
            for (k, v) in m {
                obj.insert(k, v);
            }
            Value::Object(obj)
        },
    )
}

proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(64))]

    #[test]
    fn parse_json_response_roundtrip_for_valid_json(v in arb_json_value()) {
        let raw = serde_json::to_string(&v).expect("serialize json");
        let parsed = parse_json_response(&raw, 1).expect("parse json");
        prop_assert_eq!(parsed, v);
    }

    #[test]
    fn parse_json_response_roundtrip_in_markdown_fence(v in arb_json_object()) {
        let raw = serde_json::to_string(&v).expect("serialize json");
        let wrapped = format!("Here you go:\n```json\n{raw}\n```\nDone");
        let parsed = parse_json_response(&wrapped, 1).expect("parse json");
        prop_assert_eq!(parsed, v);
    }

    #[test]
    fn token_estimator_monotonic_for_concatenation(a in ".{0,128}", b in ".{0,128}") {
        let estimator = TokenEstimator::new();
        let base = estimator.estimate_tokens(&a).estimated_tokens;
        let joined = estimator
            .estimate_tokens(&(a + &b))
            .estimated_tokens;
        prop_assert!(joined >= base);
    }
}
