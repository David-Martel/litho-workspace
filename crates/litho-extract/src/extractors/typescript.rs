use crate::extractors::Extractor;
use litho_core::types::{Dependency, Interface};
use std::path::Path;
use tree_sitter::Parser;

/// Tree-sitter–based extractor for TypeScript and TSX source files.
pub struct TypeScriptExtractor;

impl TypeScriptExtractor {
    /// Create a new [`TypeScriptExtractor`].
    pub fn new() -> Self {
        Self
    }
}

impl Default for TypeScriptExtractor {
    fn default() -> Self {
        Self::new()
    }
}

fn node_text<'a>(node: tree_sitter::Node<'_>, source_bytes: &'a [u8]) -> &'a str {
    node.utf8_text(source_bytes).unwrap_or("")
}

fn first_identifier_child(node: tree_sitter::Node<'_>, source_bytes: &[u8]) -> Option<String> {
    // Try field name first
    if let Some(n) = node.child_by_field_name("name") {
        let text = node_text(n, source_bytes);
        if !text.is_empty() {
            return Some(text.to_string());
        }
    }
    // Fall back to scanning children for an identifier node
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32)
            && (child.kind() == "identifier" || child.kind() == "type_identifier")
        {
            let text = node_text(child, source_bytes);
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }
    None
}

impl Extractor for TypeScriptExtractor {
    fn extract_interfaces(&self, content: &str, _path: &Path) -> Vec<Interface> {
        if content.is_empty() {
            return vec![];
        }

        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            .expect("Failed to load TypeScript grammar");

        let tree = match parser.parse(content, None) {
            Some(t) => t,
            None => return vec![],
        };

        let source_bytes = content.as_bytes();
        let root = tree.root_node();
        let mut interfaces = Vec::new();

        let mut stack: Vec<tree_sitter::Node> = vec![root];
        while let Some(node) = stack.pop() {
            let kind = node.kind();
            let symbol_kind = match kind {
                "function_declaration" | "arrow_function" | "generator_function_declaration" => {
                    Some("function")
                }
                "class_declaration" | "abstract_class_declaration" => Some("class"),
                "interface_declaration" => Some("interface"),
                "enum_declaration" => Some("enum"),
                "type_alias_declaration" => Some("type"),
                "export_statement" => {
                    // Don't skip — recurse into exports to find declarations.
                    let child_count = node.child_count();
                    for i in (0..child_count).rev() {
                        if let Some(child) = node.child(i as u32) {
                            stack.push(child);
                        }
                    }
                    continue;
                }
                _ => None,
            };

            if let Some(sk) = symbol_kind
                && let Some(name) = first_identifier_child(node, source_bytes)
            {
                let node_src = node_text(node, source_bytes);
                let signature = node_src
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("")
                    .trim()
                    .to_string();

                interfaces.push(Interface {
                    name,
                    kind: sk.to_string(),
                    visibility: "pub".to_string(), // TS declarations are public by default
                    signature,
                    line: node.start_position().row + 1,
                });
                continue; // do not recurse into bodies
            }

            let child_count = node.child_count();
            for i in (0..child_count).rev() {
                if let Some(child) = node.child(i as u32) {
                    stack.push(child);
                }
            }
        }

        interfaces
    }

    fn extract_dependencies(&self, content: &str, path: &Path) -> Vec<Dependency> {
        if content.is_empty() {
            return vec![];
        }

        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            .expect("Failed to load TypeScript grammar");

        let tree = match parser.parse(content, None) {
            Some(t) => t,
            None => return vec![],
        };

        let source_bytes = content.as_bytes();
        let root = tree.root_node();
        let source_path = path.to_string_lossy().to_string();
        let mut deps = Vec::new();

        let mut stack: Vec<tree_sitter::Node> = vec![root];
        while let Some(node) = stack.pop() {
            if node.kind() == "import_statement" {
                // Look for the `source` field — a string literal with the module path.
                if let Some(source_node) = node.child_by_field_name("source") {
                    let raw = node_text(source_node, source_bytes);
                    // Strip surrounding quotes.
                    let target = raw.trim_matches(|c| c == '"' || c == '\'').to_string();
                    if !target.is_empty() {
                        deps.push(Dependency {
                            source: source_path.clone(),
                            target,
                            kind: "import".to_string(),
                        });
                    }
                }
                continue;
            }

            let child_count = node.child_count();
            for i in (0..child_count).rev() {
                if let Some(child) = node.child(i as u32) {
                    stack.push(child);
                }
            }
        }

        deps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn empty_input_returns_empty() {
        let ext = TypeScriptExtractor::new();
        assert!(ext.extract_interfaces("", Path::new("index.ts")).is_empty());
        assert!(
            ext.extract_dependencies("", Path::new("index.ts"))
                .is_empty()
        );
    }

    #[test]
    fn extracts_function_and_class() {
        let code = r#"
function greet(name: string): void {
    console.log(name);
}

class Animal {
    name: string;
}
"#;
        let ext = TypeScriptExtractor::new();
        let ifaces = ext.extract_interfaces(code, Path::new("src/index.ts"));
        assert!(
            ifaces
                .iter()
                .any(|i| i.name == "greet" && i.kind == "function")
        );
        assert!(
            ifaces
                .iter()
                .any(|i| i.name == "Animal" && i.kind == "class")
        );
    }

    #[test]
    fn extracts_import() {
        let code = r#"import { readFile } from 'fs';"#;
        let ext = TypeScriptExtractor::new();
        let deps = ext.extract_dependencies(code, Path::new("src/index.ts"));
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].target, "fs");
    }
}
