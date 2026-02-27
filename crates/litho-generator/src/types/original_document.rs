use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OriginalDocument {
    /// README file content from the project repository
    pub readme: Option<String>,
    /// Supplementary project documentation (CLAUDE.md, CONTRIBUTING.md, docs/README.md)
    pub supplementary_docs: Option<String>,
    /// Technology stack extracted from manifest files (Cargo.toml, pyproject.toml, package.json, etc.)
    pub tech_stack: Option<Vec<String>>,
}

impl OriginalDocument {
    /// Combine all document content into a single formatted string for prompt consumption.
    ///
    /// This is the string that gets stored in Memory and consumed by the prompt builder.
    pub fn to_prompt_string(&self) -> String {
        let mut content = String::new();

        if let Some(ref readme) = self.readme {
            if !readme.trim().is_empty() {
                content.push_str(readme);
                content.push_str("\n\n");
            }
        }

        if let Some(ref supplementary) = self.supplementary_docs {
            if !supplementary.trim().is_empty() {
                content.push_str("---\n### Supplementary Project Documentation\n");
                content.push_str(supplementary);
                content.push_str("\n\n");
            }
        }

        if let Some(ref tech_stack) = self.tech_stack {
            if !tech_stack.is_empty() {
                content.push_str("---\n### Verified Technology Stack (extracted from manifest files)\n");
                for dep in tech_stack {
                    content.push_str("- ");
                    content.push_str(dep);
                    content.push('\n');
                }
                content.push('\n');
            }
        }

        content
    }
}
