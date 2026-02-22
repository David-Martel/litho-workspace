use litho_extract::extractors::rust::RustExtractor;
use litho_extract::extractors::Extractor;
use std::path::Path;

#[test]
fn extracts_rust_pub_functions() {
    let code = r#"
pub fn hello(name: &str) -> String {
    format!("Hello, {name}!")
}

fn private_helper() {}

pub struct Config {
    pub path: String,
    port: u16,
}

pub trait Handler: Send + Sync {
    fn handle(&self, req: Request) -> Response;
}
"#;
    let extractor = RustExtractor::new();
    let interfaces = extractor.extract_interfaces(code, Path::new("src/lib.rs"));

    // Expect 4 top-level items: hello, private_helper, Config, Handler.
    assert_eq!(
        interfaces.len(),
        4,
        "Expected 4 interfaces, got: {:?}",
        interfaces.iter().map(|i| &i.name).collect::<Vec<_>>()
    );

    let pub_fns: Vec<_> = interfaces
        .iter()
        .filter(|i| i.visibility == "pub" && i.kind == "function")
        .collect();
    assert_eq!(pub_fns.len(), 1, "Expected exactly one public function");
    assert_eq!(pub_fns[0].name, "hello");
}

#[test]
fn extracts_rust_use_dependencies() {
    let code = r#"
use std::path::PathBuf;
use crate::config::LithoConfig;
use super::types::ExtractedFile;
"#;
    let extractor = RustExtractor::new();
    let deps = extractor.extract_dependencies(code, Path::new("src/main.rs"));
    assert_eq!(deps.len(), 3, "Expected 3 use declarations");
    assert!(
        deps.iter().any(|d| d.target == "std::path::PathBuf"),
        "Missing std::path::PathBuf dependency"
    );
}
