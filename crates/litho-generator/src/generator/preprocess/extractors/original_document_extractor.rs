use crate::generator::context::GeneratorContext;
use crate::types::original_document::OriginalDocument;
use anyhow::Result;
use tokio::fs::read_to_string;

pub async fn extract(context: &GeneratorContext) -> Result<OriginalDocument> {
    let readme = match read_to_string(context.config.project_path.join("README.md")).await {
        Ok(content) => {
            let trimmed_content = trim_markdown(&content);
            Some(trimmed_content)
        }
        Err(_) => None,
    };
    Ok(OriginalDocument { readme })
}

fn trim_markdown(markdown: &str) -> String {
    let lines: Vec<&str> = markdown.lines().collect();
    let mut description = String::new();

    for line in lines.iter().take(500) {
        if line.starts_with('#') || line.starts_with("```") {
            continue;
        }
        if !line.trim().is_empty() {
            description.push_str(line);
            description.push(' ');
        }
    }

    description
}
