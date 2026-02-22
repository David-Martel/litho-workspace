pub mod csharp;
pub mod python;
pub mod rust;
pub mod typescript;

use litho_core::types::{Dependency, Interface};
use std::path::Path;

/// Language-specific AST extraction capability.
///
/// Implementors parse source text with a tree-sitter grammar and return
/// structured [`Interface`] and [`Dependency`] lists.  All methods must
/// handle empty or malformed input without panicking.
pub trait Extractor: Send + Sync {
    /// Extract top-level symbol declarations from `content`.
    fn extract_interfaces(&self, content: &str, path: &Path) -> Vec<Interface>;

    /// Extract import / use declarations from `content`.
    fn extract_dependencies(&self, content: &str, path: &Path) -> Vec<Dependency>;
}
