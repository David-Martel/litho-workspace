use crate::extractors::Extractor;
use litho_core::types::{Dependency, Interface};
use std::path::Path;
use tree_sitter::Parser;

/// Tree-sitter–based extractor for Rust source files.
pub struct RustExtractor;

impl RustExtractor {
    /// Create a new [`RustExtractor`].
    pub fn new() -> Self {
        Self
    }
}

impl Default for RustExtractor {
    fn default() -> Self {
        Self::new()
    }
}

/// Return the text of `node` inside `source_bytes`, or an empty string on
/// encoding errors.
fn node_text<'a>(node: tree_sitter::Node<'_>, source_bytes: &'a [u8]) -> &'a str {
    node.utf8_text(source_bytes).unwrap_or("")
}

/// Check whether the first child of `node` is a `visibility_modifier` and
/// return its text if so, otherwise return `"private"`.
fn visibility(node: tree_sitter::Node<'_>, source_bytes: &[u8]) -> String {
    if node.child_count() == 0 {
        return "private".to_string();
    }
    if let Some(first) = node.child(0)
        && first.kind() == "visibility_modifier"
    {
        return node_text(first, source_bytes).to_string();
    }
    "private".to_string()
}

/// Extract the identifier child named `"name"` from a declaration node.
fn extract_name<'a>(node: tree_sitter::Node<'_>, source_bytes: &'a [u8]) -> Option<&'a str> {
    // Try the `name` field first (works for function_item, struct_item, etc.)
    if let Some(name_node) = node.child_by_field_name("name") {
        return Some(node_text(name_node, source_bytes));
    }
    None
}

impl Extractor for RustExtractor {
    fn extract_interfaces(&self, content: &str, _path: &Path) -> Vec<Interface> {
        if content.is_empty() {
            return vec![];
        }

        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .expect("Failed to load Rust grammar");

        let tree = match parser.parse(content, None) {
            Some(t) => t,
            None => return vec![],
        };

        let source_bytes = content.as_bytes();
        let root = tree.root_node();
        let mut interfaces = Vec::new();

        // We do a full depth-first traversal looking for top-level declarations.
        // Using a manual stack avoids lifetime conflicts with the tree-sitter
        // cursor API when we need to collect mutable results.
        let mut stack: Vec<tree_sitter::Node> = vec![root];

        while let Some(node) = stack.pop() {
            let kind = node.kind();

            match kind {
                "function_item" | "struct_item" | "enum_item" | "trait_item" | "impl_item"
                | "type_item" => {
                    if let Some(name) = extract_name(node, source_bytes) {
                        if name.is_empty() {
                            // Fall through to children
                        } else {
                            let vis = visibility(node, source_bytes);

                            // Determine the symbol kind.
                            let symbol_kind = match kind {
                                "function_item" => "function",
                                "struct_item" => "struct",
                                "enum_item" => "enum",
                                "trait_item" => "trait",
                                "impl_item" => "impl",
                                "type_item" => "type",
                                _ => kind,
                            };

                            // Signature: first non-empty line of the node text.
                            let node_src = node_text(node, source_bytes);
                            let signature = node_src
                                .lines()
                                .find(|l| !l.trim().is_empty())
                                .unwrap_or("")
                                .trim()
                                .to_string();

                            let line = node.start_position().row as usize + 1; // 1-based

                            interfaces.push(Interface {
                                name: name.to_string(),
                                kind: symbol_kind.to_string(),
                                visibility: vis,
                                signature,
                                line,
                            });
                            // Do NOT recurse into item bodies — we only want
                            // top-level symbols.
                            continue;
                        }
                    }
                }
                _ => {}
            }

            // Push children in reverse order so they are processed left-to-right.
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
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .expect("Failed to load Rust grammar");

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
            if node.kind() == "use_declaration" {
                // Get the full text of the use declaration and strip "use " prefix
                // and ";" suffix to get the import path.
                let text = node_text(node, source_bytes).trim().to_string();
                let target = text
                    .strip_prefix("use ")
                    .unwrap_or(&text)
                    .trim_end_matches(';')
                    .trim()
                    .to_string();

                if !target.is_empty() {
                    deps.push(Dependency {
                        source: source_path.clone(),
                        target,
                        kind: "use".to_string(),
                    });
                }
                continue; // no need to recurse into use_declaration internals
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
        let ext = RustExtractor::new();
        assert!(ext.extract_interfaces("", Path::new("lib.rs")).is_empty());
        assert!(ext.extract_dependencies("", Path::new("lib.rs")).is_empty());
    }

    #[test]
    fn extracts_visibility() {
        let code = r#"
pub fn public_fn() {}
fn private_fn() {}
pub(crate) fn crate_fn() {}
"#;
        let ext = RustExtractor::new();
        let ifaces = ext.extract_interfaces(code, Path::new("src/lib.rs"));
        assert_eq!(ifaces.len(), 3);

        let public = ifaces.iter().find(|i| i.name == "public_fn").unwrap();
        assert_eq!(public.visibility, "pub");

        let private = ifaces.iter().find(|i| i.name == "private_fn").unwrap();
        assert_eq!(private.visibility, "private");
    }
}
