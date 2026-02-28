use crate::generator::{compose::memory::MemoryScope, context::GeneratorContext};
use anyhow::Result;
use pulldown_cmark::{html, Options, Parser};
use std::fs;

use super::{DocTree, MarkdownFixer, MermaidFixer, Outlet};

/// Outlet that converts markdown documents to HTML and writes them to disk.
pub struct HtmlOutlet {
    doc_tree: DocTree,
}

impl HtmlOutlet {
    pub fn new(doc_tree: DocTree) -> Self {
        Self { doc_tree }
    }
}

impl Outlet for HtmlOutlet {
    async fn save(&self, context: &GeneratorContext) -> Result<()> {
        println!("\n🖊️ Saving documentation as HTML...");

        let output_dir = &context.config.output_path;
        if output_dir.exists() {
            fs::remove_dir_all(output_dir)?;
        }
        fs::create_dir_all(output_dir)?;

        let project_name = context
            .config
            .project_name
            .as_deref()
            .unwrap_or("Project");

        for (scoped_key, relative_path) in &self.doc_tree.structure {
            if let Some(doc_markdown) = context
                .get_from_memory::<String>(MemoryScope::DOCUMENTATION, scoped_key)
                .await
            {
                // Apply structural markdown fixes before HTML conversion
                let (fixed_markdown, fix_report) = MarkdownFixer::fix(&doc_markdown);
                if fix_report.total_fixes() > 0 {
                    println!(
                        "  [md-fixer] {}: {} fixes applied before HTML conversion",
                        scoped_key,
                        fix_report.total_fixes(),
                    );
                }

                // Convert .md extension to .html
                let html_path = if relative_path.ends_with(".md") {
                    format!("{}.html", &relative_path[..relative_path.len() - 3])
                } else {
                    format!("{}.html", relative_path)
                };

                let output_file_path = output_dir.join(&html_path);

                if let Some(parent_dir) = output_file_path.parent() {
                    if !parent_dir.exists() {
                        fs::create_dir_all(parent_dir)?;
                    }
                }

                let html_content = markdown_to_html(&fixed_markdown, project_name, scoped_key);
                fs::write(&output_file_path, html_content)?;

                println!("💾 HTML document saved: {}", output_file_path.display());
            } else {
                let msg = context.config.target_language.msg_doc_not_found();
                eprintln!("{}", msg.replace("{}", scoped_key));
            }
        }

        println!(
            "💾 HTML document save completed, output directory: {}",
            output_dir.display()
        );

        // Mermaid fixer still runs (for any markdown-based fixups before conversion)
        if let Err(e) = MermaidFixer::auto_fix_after_output(context).await {
            let msg = context.config.target_language.msg_mermaid_error();
            eprintln!("{}", msg.replace("{}", &e.to_string()));
            eprintln!("💡 This will not affect the main documentation generation process");
        }

        Ok(())
    }
}

/// Convert markdown content to a complete HTML document with minimal styling.
fn markdown_to_html(markdown: &str, project_name: &str, title: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(markdown, options);
    let mut html_body = String::new();
    html::push_html(&mut html_body, parser);

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} — {project_name}</title>
<style>
body {{
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
  line-height: 1.6;
  max-width: 900px;
  margin: 0 auto;
  padding: 2rem;
  color: #333;
}}
h1, h2, h3, h4 {{ color: #1a1a2e; margin-top: 1.5em; }}
h1 {{ border-bottom: 2px solid #e0e0e0; padding-bottom: 0.3em; }}
h2 {{ border-bottom: 1px solid #e0e0e0; padding-bottom: 0.2em; }}
code {{
  background: #f5f5f5;
  padding: 0.2em 0.4em;
  border-radius: 3px;
  font-size: 0.9em;
}}
pre {{
  background: #f5f5f5;
  padding: 1rem;
  border-radius: 6px;
  overflow-x: auto;
}}
pre code {{ background: transparent; padding: 0; }}
table {{
  border-collapse: collapse;
  width: 100%;
  margin: 1em 0;
}}
th, td {{
  border: 1px solid #ddd;
  padding: 0.5em 0.75em;
  text-align: left;
}}
th {{ background: #f5f5f5; font-weight: 600; }}
tr:nth-child(even) {{ background: #fafafa; }}
blockquote {{
  border-left: 4px solid #ddd;
  margin: 1em 0;
  padding: 0.5em 1em;
  color: #666;
}}
a {{ color: #0066cc; }}
nav {{ margin-bottom: 2rem; font-size: 0.9em; color: #666; }}
nav a {{ margin-right: 1em; }}
</style>
</head>
<body>
<nav><a href="index.html">← Index</a></nav>
{html_body}
<footer style="margin-top:3rem;padding-top:1rem;border-top:1px solid #eee;font-size:0.85em;color:#999;">
Generated by <a href="https://github.com/nicepkg/litho">Litho</a>
</footer>
</body>
</html>"#,
        title = title,
        project_name = project_name,
        html_body = html_body,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_markdown_to_html_basic() {
        let md = "# Hello World\n\nThis is a **test**.";
        let html = markdown_to_html(md, "TestProject", "Hello");
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("<h1>Hello World</h1>"));
        assert!(html.contains("<strong>test</strong>"));
        assert!(html.contains("TestProject"));
    }

    #[test]
    fn test_markdown_to_html_table() {
        let md = "| Col A | Col B |\n|-------|-------|\n| 1 | 2 |";
        let html = markdown_to_html(md, "Proj", "Table");
        assert!(html.contains("<table>"));
        assert!(html.contains("<th>Col A</th>"));
        assert!(html.contains("<td>1</td>"));
    }

    #[test]
    fn test_markdown_to_html_code_block() {
        let md = "```rust\nfn main() {}\n```";
        let html = markdown_to_html(md, "Proj", "Code");
        assert!(html.contains("<pre>"));
        assert!(html.contains("fn main()"));
    }
}
