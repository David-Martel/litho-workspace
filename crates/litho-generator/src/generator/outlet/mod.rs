use crate::generator::compose::types::AgentType;
use crate::generator::{compose::memory::MemoryScope, context::GeneratorContext};
use crate::i18n::TargetLanguage;
use anyhow::Result;
use std::collections::HashMap;
use std::fs;

pub mod fixer;
pub mod html_outlet;
pub mod summary_generator;
pub mod summary_outlet;

pub use fixer::MermaidFixer;
pub use html_outlet::HtmlOutlet;
pub use summary_outlet::SummaryOutlet;

pub trait Outlet {
    async fn save(&self, context: &GeneratorContext) -> Result<()>;
}

/// Enum-based dispatch over all format-dependent outlet variants.
///
/// Because `Outlet::save` is an async trait method (RPITIT), the trait is not
/// object-safe and cannot be used as `Box<dyn Outlet>`. `OutletKind` wraps
/// every concrete outlet and delegates to the appropriate implementation,
/// giving callers a single type that erases the concrete variant without
/// requiring dynamic dispatch through a vtable.
///
/// # Examples
///
/// ```ignore
/// let outlet = OutletKind::for_format("html", doc_tree);
/// outlet.save(&context).await?;
/// ```
pub enum OutletKind {
    Disk(DiskOutlet),
    Html(HtmlOutlet),
}

impl OutletKind {
    /// Construct the appropriate outlet variant for `format`.
    ///
    /// `"html"` maps to [`HtmlOutlet`]; every other string (including `"md"`)
    /// maps to [`DiskOutlet`].
    pub fn for_format(format: &str, doc_tree: DocTree) -> Self {
        match format {
            "html" => Self::Html(HtmlOutlet::new(doc_tree)),
            _ => Self::Disk(DiskOutlet::new(doc_tree)),
        }
    }

    /// Save documentation using whichever outlet variant is active.
    pub async fn save(&self, context: &GeneratorContext) -> Result<()> {
        match self {
            Self::Disk(o) => o.save(context).await,
            Self::Html(o) => o.save(context).await,
        }
    }
}

pub struct DocTree {
    /// key is the ScopedKey of Documentation in Memory, value is the relative path for document output
    structure: HashMap<String, String>,
}

impl DocTree {
    pub fn new(target_language: &TargetLanguage) -> Self {
        let structure = HashMap::from([
            (
                AgentType::Overview.to_string(),
                target_language.get_doc_filename("overview"),
            ),
            (
                AgentType::Architecture.to_string(),
                target_language.get_doc_filename("architecture"),
            ),
            (
                AgentType::Workflow.to_string(),
                target_language.get_doc_filename("workflow"),
            ),
            (
                AgentType::Boundary.to_string(),
                target_language.get_doc_filename("boundary"),
            ),
            (
                AgentType::Database.to_string(),
                target_language.get_doc_filename("database"),
            ),
        ]);
        Self { structure }
    }

    pub fn insert(&mut self, scoped_key: &str, relative_path: &str) {
        self.structure
            .insert(scoped_key.to_string(), relative_path.to_string());
    }
}

impl Default for DocTree {
    fn default() -> Self {
        // Default to English
        Self::new(&TargetLanguage::English)
    }
}

pub struct DiskOutlet {
    doc_tree: DocTree,
}

impl DiskOutlet {
    pub fn new(doc_tree: DocTree) -> Self {
        Self { doc_tree }
    }
}

impl Outlet for DiskOutlet {
    async fn save(&self, context: &GeneratorContext) -> Result<()> {
        println!("\nSaving documentation...");
        // Create output directory
        let output_dir = &context.config.output_path;
        if output_dir.exists() {
            fs::remove_dir_all(output_dir)?;
        }
        fs::create_dir_all(output_dir)?;

        // Iterate through document tree structure and save each document
        for (scoped_key, relative_path) in &self.doc_tree.structure {
            // Get document content from memory
            if let Some(doc_markdown) = context
                .get_from_memory::<String>(MemoryScope::DOCUMENTATION, scoped_key)
                .await
            {
                // Build full output file path
                let output_file_path = output_dir.join(relative_path);

                // Ensure parent directory exists
                if let Some(parent_dir) = output_file_path.parent() {
                    if !parent_dir.exists() {
                        fs::create_dir_all(parent_dir)?;
                    }
                }

                // Write document content to file
                fs::write(&output_file_path, doc_markdown)?;

                println!("Document saved: {}", output_file_path.display());
            } else {
                // If document doesn't exist, log warning but don't interrupt the process
                let msg = context.config.target_language.msg_doc_not_found();
                eprintln!("{}", msg.replace("{}", scoped_key));
            }
        }

        println!(
            "Document save completed, output directory: {}",
            output_dir.display()
        );

        // Automatically fix mermaid charts after document save
        if let Err(e) = MermaidFixer::auto_fix_after_output(context).await {
            let msg = context.config.target_language.msg_mermaid_error();
            eprintln!("{}", msg.replace("{}", &e.to_string()));
            eprintln!("This will not affect the main documentation generation process");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify that `OutletKind::for_format` returns the expected variant for
    /// every documented format string.
    #[test]
    fn test_for_format_html_variant() {
        let doc_tree = DocTree::default();
        let outlet = OutletKind::for_format("html", doc_tree);
        assert!(
            matches!(outlet, OutletKind::Html(_)),
            "expected Html variant for format \"html\""
        );
    }

    #[test]
    fn test_for_format_md_variant() {
        let doc_tree = DocTree::default();
        let outlet = OutletKind::for_format("md", doc_tree);
        assert!(
            matches!(outlet, OutletKind::Disk(_)),
            "expected Disk variant for format \"md\""
        );
    }

    #[test]
    fn test_for_format_unknown_falls_back_to_disk() {
        let doc_tree = DocTree::default();
        let outlet = OutletKind::for_format("pdf", doc_tree);
        assert!(
            matches!(outlet, OutletKind::Disk(_)),
            "expected Disk variant for unknown format \"pdf\""
        );
    }

    #[test]
    fn test_for_format_empty_string_falls_back_to_disk() {
        let doc_tree = DocTree::default();
        let outlet = OutletKind::for_format("", doc_tree);
        assert!(
            matches!(outlet, OutletKind::Disk(_)),
            "expected Disk variant for empty format string"
        );
    }
}
