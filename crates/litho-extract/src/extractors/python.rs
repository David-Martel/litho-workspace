use crate::extractors::Extractor;
use litho_core::types::{Dependency, Interface};
use std::path::Path;
use tree_sitter::Parser;

/// Tree-sitter–based extractor for Python source files.
pub struct PythonExtractor;

impl PythonExtractor {
    /// Create a new [`PythonExtractor`].
    pub fn new() -> Self {
        Self
    }
}

impl Default for PythonExtractor {
    fn default() -> Self {
        Self::new()
    }
}

fn node_text<'a>(node: tree_sitter::Node<'_>, source_bytes: &'a [u8]) -> &'a str {
    node.utf8_text(source_bytes).unwrap_or("")
}

fn identifier_child(node: tree_sitter::Node<'_>, source_bytes: &[u8]) -> Option<String> {
    if let Some(n) = node.child_by_field_name("name") {
        let t = node_text(n, source_bytes);
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as usize)
            && child.kind() == "identifier"
        {
            let t = node_text(child, source_bytes);
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    None
}

impl Extractor for PythonExtractor {
    fn extract_interfaces(&self, content: &str, _path: &Path) -> Vec<Interface> {
        if content.is_empty() {
            return vec![];
        }

        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .expect("Failed to load Python grammar");

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
                "function_definition" => Some("function"),
                "class_definition" => Some("class"),
                "decorated_definition" => {
                    // Recurse into decorated definitions to find the actual def.
                    let child_count = node.child_count();
                    for i in (0..child_count).rev() {
                        if let Some(child) = node.child(i as usize) {
                            stack.push(child);
                        }
                    }
                    continue;
                }
                _ => None,
            };

            if let Some(sk) = symbol_kind
                && let Some(name) = identifier_child(node, source_bytes)
            {
                let node_src = node_text(node, source_bytes);
                let signature = node_src
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("")
                    .trim()
                    .to_string();

                // Rough visibility: starts with "_" → private.
                let vis = if name.starts_with('_') {
                    "private"
                } else {
                    "pub"
                };

                interfaces.push(Interface {
                    name,
                    kind: sk.to_string(),
                    visibility: vis.to_string(),
                    signature,
                    line: node.start_position().row as usize + 1,
                });
                continue;
            }

            let child_count = node.child_count();
            for i in (0..child_count).rev() {
                if let Some(child) = node.child(i as usize) {
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
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .expect("Failed to load Python grammar");

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
            match node.kind() {
                "import_statement" => {
                    // "import foo.bar, baz"
                    let text = node_text(node, source_bytes);
                    let target = text
                        .strip_prefix("import ")
                        .unwrap_or(text)
                        .trim()
                        .to_string();
                    if !target.is_empty() {
                        deps.push(Dependency {
                            source: source_path.clone(),
                            target,
                            kind: "import".to_string(),
                        });
                    }
                    continue;
                }
                "import_from_statement" => {
                    // "from foo.bar import baz"
                    let text = node_text(node, source_bytes);
                    deps.push(Dependency {
                        source: source_path.clone(),
                        target: text.trim().to_string(),
                        kind: "from_import".to_string(),
                    });
                    continue;
                }
                _ => {}
            }

            let child_count = node.child_count();
            for i in (0..child_count).rev() {
                if let Some(child) = node.child(i as usize) {
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
        let ext = PythonExtractor::new();
        assert!(ext.extract_interfaces("", Path::new("mod.py")).is_empty());
        assert!(ext.extract_dependencies("", Path::new("mod.py")).is_empty());
    }

    #[test]
    fn extracts_function_and_class() {
        let code = r#"
def greet(name):
    return f"Hello, {name}"

class Animal:
    def __init__(self, name):
        self.name = name
"#;
        let ext = PythonExtractor::new();
        let ifaces = ext.extract_interfaces(code, Path::new("src/mod.py"));
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
    fn extracts_imports() {
        let code = "import os\nfrom pathlib import Path\n";
        let ext = PythonExtractor::new();
        let deps = ext.extract_dependencies(code, Path::new("script.py"));
        assert_eq!(deps.len(), 2);
    }
}
