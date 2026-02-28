use crate::extractors::Extractor;
use litho_core::types::{Dependency, Interface};
use std::path::Path;
use tree_sitter::Parser;

/// Tree-sitter–based extractor for C# source files.
pub struct CSharpExtractor;

impl CSharpExtractor {
    /// Create a new [`CSharpExtractor`].
    pub fn new() -> Self {
        Self
    }
}

impl Default for CSharpExtractor {
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
        if let Some(child) = node.child(i)
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

/// Extract the visibility from C# access modifiers that appear as children.
fn cs_visibility(node: tree_sitter::Node<'_>, source_bytes: &[u8]) -> String {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            let k = child.kind();
            if k == "modifier" {
                let t = node_text(child, source_bytes);
                if matches!(t, "public" | "protected" | "internal" | "private") {
                    return t.to_string();
                }
            }
        }
    }
    "private".to_string()
}

impl Extractor for CSharpExtractor {
    fn extract_interfaces(&self, content: &str, _path: &Path) -> Vec<Interface> {
        if content.is_empty() {
            return vec![];
        }

        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_c_sharp::LANGUAGE.into())
            .expect("Failed to load C# grammar");

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
                "method_declaration" | "local_function_statement" => Some("function"),
                "class_declaration" | "record_declaration" => Some("class"),
                "interface_declaration" => Some("interface"),
                "struct_declaration" => Some("struct"),
                "enum_declaration" => Some("enum"),
                _ => None,
            };

            if let Some(sk) = symbol_kind
                && let Some(name) = identifier_child(node, source_bytes)
            {
                let vis = cs_visibility(node, source_bytes);
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
                    visibility: vis,
                    signature,
                    line: node.start_position().row + 1,
                });
                // Do not recurse into members of a class/interface to keep
                // the list at the declared-type level.  Recurse for
                // namespace_declaration and compilation_unit.
                continue;
            }

            let child_count = node.child_count();
            for i in (0..child_count).rev() {
                if let Some(child) = node.child(i) {
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
            .set_language(&tree_sitter_c_sharp::LANGUAGE.into())
            .expect("Failed to load C# grammar");

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
            if node.kind() == "using_directive" {
                let text = node_text(node, source_bytes).trim().to_string();
                // Strip "using " prefix and ";" suffix.
                let target = text
                    .strip_prefix("using ")
                    .unwrap_or(&text)
                    .trim_end_matches(';')
                    .trim()
                    .to_string();
                if !target.is_empty() {
                    deps.push(Dependency {
                        source: source_path.clone(),
                        target,
                        kind: "using".to_string(),
                    });
                }
                continue;
            }

            let child_count = node.child_count();
            for i in (0..child_count).rev() {
                if let Some(child) = node.child(i) {
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
        let ext = CSharpExtractor::new();
        assert!(ext.extract_interfaces("", Path::new("Foo.cs")).is_empty());
        assert!(ext.extract_dependencies("", Path::new("Foo.cs")).is_empty());
    }

    #[test]
    fn extracts_class() {
        let code = r#"
public class Greeter {
    public string Greet(string name) => $"Hello, {name}";
}
"#;
        let ext = CSharpExtractor::new();
        let ifaces = ext.extract_interfaces(code, Path::new("Greeter.cs"));
        assert!(
            ifaces
                .iter()
                .any(|i| i.name == "Greeter" && i.kind == "class")
        );
    }

    #[test]
    fn extracts_using() {
        let code = "using System;\nusing System.IO;\n";
        let ext = CSharpExtractor::new();
        let deps = ext.extract_dependencies(code, Path::new("Program.cs"));
        assert_eq!(deps.len(), 2);
        assert!(deps.iter().any(|d| d.target == "System"));
    }
}
