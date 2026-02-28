use comrak::nodes::{AstNode, NodeValue};
use comrak::options::Extension;
use comrak::{Arena, Options, format_commonmark, parse_document};

/// Structural markdown fixer using comrak's AST.
///
/// Parses markdown into a full AST, applies a series of structural fixes,
/// and renders back to CommonMark. This runs before file output and before
/// the external MermaidFixer subprocess.
pub struct MarkdownFixer;

/// Report of fixes applied by [`MarkdownFixer::fix`].
#[derive(Debug, Default, Clone)]
pub struct FixReport {
    pub headings_fixed: usize,
    pub links_fixed: usize,
    pub mermaid_blocks_found: usize,
    pub tables_found: usize,
    pub empty_headings_removed: usize,
}

impl FixReport {
    /// Total number of modifications made.
    pub fn total_fixes(&self) -> usize {
        self.headings_fixed + self.links_fixed + self.empty_headings_removed
    }
}

impl MarkdownFixer {
    /// Parse → transform → render back to CommonMark.
    ///
    /// Returns the fixed markdown and a report of what was changed.
    pub fn fix(input: &str) -> (String, FixReport) {
        let arena = Arena::new();
        let opts = Self::options();

        let root = parse_document(&arena, input, &opts);
        let mut report = FixReport::default();

        Self::enforce_single_h1(root, &mut report);
        Self::fix_empty_links(root, &mut report);
        Self::remove_empty_headings(root, &mut report);
        Self::audit_mermaid_blocks(root, &mut report);
        Self::audit_tables(root, &mut report);

        let mut out = String::with_capacity(input.len());
        format_commonmark(root, &opts, &mut out).unwrap();
        (out, report)
    }

    /// Build comrak options with GFM extensions enabled.
    fn options() -> Options<'static> {
        Options {
            extension: Extension {
                strikethrough: true,
                table: true,
                tasklist: true,
                autolink: true,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Ensure only one H1 exists; demote extras to H2.
    fn enforce_single_h1<'a>(root: &'a AstNode<'a>, report: &mut FixReport) {
        let mut h1_seen = false;
        for node in root.descendants() {
            let mut data = node.data.borrow_mut();
            if let NodeValue::Heading(ref mut heading) = data.value
                && heading.level == 1
            {
                if h1_seen {
                    heading.level = 2;
                    report.headings_fixed += 1;
                } else {
                    h1_seen = true;
                }
            }
        }
    }

    /// Replace links with empty href with a `#` placeholder.
    fn fix_empty_links<'a>(root: &'a AstNode<'a>, report: &mut FixReport) {
        for node in root.descendants() {
            let mut data = node.data.borrow_mut();
            if let NodeValue::Link(ref mut link) = data.value
                && link.url.is_empty()
            {
                link.url = "#".to_string();
                report.links_fixed += 1;
            }
        }
    }

    /// Remove heading nodes that have no text content (LLM artifacts).
    fn remove_empty_headings<'a>(root: &'a AstNode<'a>, report: &mut FixReport) {
        let mut to_remove = Vec::new();
        for node in root.descendants() {
            let data = node.data.borrow();
            if matches!(data.value, NodeValue::Heading(_)) {
                drop(data);
                // Check if the heading has any text children
                let has_text = node.descendants().any(|child| {
                    let cd = child.data.borrow();
                    matches!(&cd.value, NodeValue::Text(t) if !t.trim().is_empty())
                        || matches!(cd.value, NodeValue::Code(_))
                });
                if !has_text {
                    to_remove.push(node);
                }
            }
        }
        for node in to_remove {
            node.detach();
            report.empty_headings_removed += 1;
        }
    }

    /// Count Mermaid fenced code blocks for the report.
    fn audit_mermaid_blocks<'a>(root: &'a AstNode<'a>, report: &mut FixReport) {
        for node in root.descendants() {
            let data = node.data.borrow();
            if let NodeValue::CodeBlock(ref cb) = data.value
                && cb.info.eq_ignore_ascii_case("mermaid")
            {
                report.mermaid_blocks_found += 1;
            }
        }
    }

    /// Count table nodes for audit purposes.
    fn audit_tables<'a>(root: &'a AstNode<'a>, report: &mut FixReport) {
        for node in root.descendants() {
            let data = node.data.borrow();
            if matches!(data.value, NodeValue::Table(_)) {
                report.tables_found += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_h1_preserved() {
        let input = "# Title\n\nSome text\n";
        let (output, report) = MarkdownFixer::fix(input);
        assert!(output.contains("# Title"));
        assert_eq!(report.headings_fixed, 0);
    }

    #[test]
    fn test_duplicate_h1_demoted() {
        let input = "# First\n\n# Second\n\n# Third\n";
        let (output, report) = MarkdownFixer::fix(input);
        assert!(output.contains("# First"));
        assert!(output.contains("## Second"));
        assert!(output.contains("## Third"));
        assert_eq!(report.headings_fixed, 2);
    }

    #[test]
    fn test_empty_link_fixed() {
        let input = "Click [here]() for details.\n";
        let (output, report) = MarkdownFixer::fix(input);
        assert!(output.contains("[here](#)"));
        assert_eq!(report.links_fixed, 1);
    }

    #[test]
    fn test_valid_link_unchanged() {
        let input = "See [docs](https://example.com).\n";
        let (output, report) = MarkdownFixer::fix(input);
        assert!(output.contains("https://example.com"));
        assert_eq!(report.links_fixed, 0);
    }

    #[test]
    fn test_mermaid_block_detected() {
        let input = "# Arch\n\n```mermaid\ngraph TD\n  A-->B\n```\n";
        let (_, report) = MarkdownFixer::fix(input);
        assert_eq!(report.mermaid_blocks_found, 1);
    }

    #[test]
    fn test_table_detected() {
        let input = "| Col | Val |\n|-----|-----|\n| a   | b   |\n";
        let (_, report) = MarkdownFixer::fix(input);
        assert_eq!(report.tables_found, 1);
    }

    #[test]
    fn test_no_fixes_empty_report() {
        let input = "# Title\n\nJust some text.\n";
        let (_, report) = MarkdownFixer::fix(input);
        assert_eq!(report.total_fixes(), 0);
    }

    #[test]
    fn test_complex_document() {
        let input = r#"# Overview

## Architecture

Some text with a [broken]() link and a [good](https://x.com) one.

# Duplicate Title

```mermaid
graph LR
  A-->B
```

| Key | Value |
|-----|-------|
| foo | bar   |
"#;
        let (output, report) = MarkdownFixer::fix(input);
        assert_eq!(report.headings_fixed, 1);
        assert_eq!(report.links_fixed, 1);
        assert_eq!(report.mermaid_blocks_found, 1);
        assert_eq!(report.tables_found, 1);
        // Verify the duplicate H1 was demoted
        assert!(output.contains("## Duplicate Title"));
    }
}
