use super::{Dependency, LanguageProcessor};
use crate::types::code::InterfaceInfo;
use regex::Regex;
use std::path::Path;

#[derive(Debug)]
pub struct SvelteProcessor {
    script_regex: Regex,
    import_regex: Regex,
}

impl SvelteProcessor {
    pub fn new() -> Self {
        Self {
            script_regex: Regex::new(r"<script[^>]*>(.*?)</script>").unwrap(),
            import_regex: Regex::new(r#"^\s*import\s+(?:.*\s+from\s+)?['"]([^'"]+)['"]"#).unwrap(),
        }
    }

    fn extract_script_content(&self, content: &str) -> String {
        if let Some(captures) = self.script_regex.captures(content) {
            if let Some(script_content) = captures.get(1) {
                return script_content.as_str().to_string();
            }
        }
        content.to_string()
    }
}

impl LanguageProcessor for SvelteProcessor {
    fn supported_extensions(&self) -> Vec<&'static str> {
        vec!["svelte"]
    }

    fn extract_dependencies(&self, content: &str, file_path: &Path) -> Vec<Dependency> {
        let mut dependencies = Vec::new();
        let script_content = self.extract_script_content(content);
        let source_file = file_path.to_string_lossy().to_string();

        for (line_num, line) in script_content.lines().enumerate() {
            if let Some(captures) = self.import_regex.captures(line) {
                if let Some(import_path) = captures.get(1) {
                    let path_str = import_path.as_str();
                    let is_external = !path_str.starts_with('.')
                        && !path_str.starts_with('/')
                        && !path_str.starts_with('$');

                    let dependency_type = if path_str.starts_with("svelte") {
                        "svelte_import"
                    } else if path_str.ends_with(".svelte") {
                        "svelte_component_import"
                    } else if path_str.starts_with('$') {
                        "svelte_store_import"
                    } else {
                        "import"
                    };

                    dependencies.push(Dependency {
                        name: source_file.clone(),
                        path: Some(path_str.to_string()),
                        is_external,
                        line_number: Some(line_num + 1),
                        dependency_type: dependency_type.to_string(),
                        version: None,
                    });
                }
            }
        }

        dependencies
    }

    fn determine_component_type(&self, file_path: &Path, content: &str) -> String {
        let file_name = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // Check special file names
        if file_name == "App.svelte" {
            return "svelte_app".to_string();
        }

        if file_name == "index.svelte" {
            return "svelte_entry".to_string();
        }

        if file_name.to_lowercase().contains("page")
            || file_path.to_string_lossy().contains("/routes/")
        {
            return "svelte_page".to_string();
        }

        if file_name.to_lowercase().contains("layout") {
            return "svelte_layout".to_string();
        }

        // Check content patterns
        if content.contains("<script>") && content.contains("export") {
            if content.contains("export let") {
                "svelte_component".to_string()
            } else {
                "svelte_module".to_string()
            }
        } else if content.contains("writable")
            || content.contains("readable")
            || content.contains("derived")
        {
            "svelte_store".to_string()
        } else {
            "svelte_file".to_string()
        }
    }

    fn is_important_line(&self, line: &str) -> bool {
        let trimmed = line.trim();

        // Svelte tags
        if trimmed.starts_with("<script>") || trimmed.starts_with("<style>") {
            return true;
        }

        // Svelte-specific syntax
        if trimmed.starts_with("export let ") || trimmed.contains("$:") {
            return true;
        }

        // Svelte stores
        if trimmed.contains("writable(")
            || trimmed.contains("readable(")
            || trimmed.contains("derived(")
            || trimmed.contains("$")
        {
            return true;
        }

        // Import statements
        if trimmed.starts_with("import ") {
            return true;
        }

        // Svelte directives
        if trimmed.contains("on:")
            || trimmed.contains("bind:")
            || trimmed.contains("use:")
            || trimmed.contains("transition:")
            || trimmed.contains("in:")
            || trimmed.contains("out:")
        {
            return true;
        }

        // Conditionals and loops
        if trimmed.contains("{#if")
            || trimmed.contains("{#each")
            || trimmed.contains("{#await")
            || trimmed.contains("{/if")
            || trimmed.contains("{/each")
            || trimmed.contains("{/await")
        {
            return true;
        }

        // Important comments
        if trimmed.contains("TODO")
            || trimmed.contains("FIXME")
            || trimmed.contains("NOTE")
            || trimmed.contains("HACK")
        {
            return true;
        }

        false
    }

    fn language_name(&self) -> &'static str {
        "Svelte"
    }

    fn extract_interfaces(&self, content: &str, _file_path: &Path) -> Vec<InterfaceInfo> {
        let mut interfaces = Vec::new();

        // Svelte component interface analysis
        interfaces.push(InterfaceInfo {
            name: "SvelteComponent".to_string(),
            interface_type: "svelte_component".to_string(),
            visibility: "public".to_string(),
            parameters: Vec::new(),
            return_type: None,
            description: Some("Svelte single file component".to_string()),
        });

        // Extract functions in script tag
        if content.contains("<script") {
            let lines: Vec<&str> = content.lines().collect();
            for line in lines {
                let trimmed = line.trim();

                // Extract function definitions
                if trimmed.starts_with("function ") || trimmed.contains("= function") {
                    if let Some(func_name) = self.extract_svelte_function(trimmed) {
                        interfaces.push(InterfaceInfo {
                            name: func_name,
                            interface_type: "svelte_function".to_string(),
                            visibility: "public".to_string(),
                            parameters: Vec::new(),
                            return_type: None,
                            description: None,
                        });
                    }
                }

                // Extract reactive declarations
                if trimmed.starts_with("$:") {
                    interfaces.push(InterfaceInfo {
                        name: "reactive_statement".to_string(),
                        interface_type: "svelte_reactive".to_string(),
                        visibility: "public".to_string(),
                        parameters: Vec::new(),
                        return_type: None,
                        description: Some("Svelte reactive declaration".to_string()),
                    });
                }
            }
        }

        interfaces
    }
}

impl SvelteProcessor {
    /// Extract Svelte function name
    fn extract_svelte_function(&self, line: &str) -> Option<String> {
        if line.contains("function ") {
            if let Some(start) = line.find("function ") {
                let after_function = &line[start + 9..];
                if let Some(paren_pos) = after_function.find('(') {
                    let func_name = after_function[..paren_pos].trim();
                    if !func_name.is_empty() {
                        return Some(func_name.to_string());
                    }
                }
            }
        } else if line.contains("= function") {
            if let Some(eq_pos) = line.find('=') {
                let before_eq = &line[..eq_pos].trim();
                if let Some(space_pos) = before_eq.rfind(' ') {
                    let func_name = before_eq[space_pos + 1..].trim();
                    if !func_name.is_empty() {
                        return Some(func_name.to_string());
                    }
                }
            }
        }
        None
    }
}