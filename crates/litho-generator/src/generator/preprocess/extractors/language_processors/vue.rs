use super::{Dependency, LanguageProcessor};
use crate::types::code::InterfaceInfo;
use regex::Regex;
use std::path::Path;

#[derive(Debug)]
pub struct VueProcessor {
    script_regex: Regex,
    import_regex: Regex,
}

impl VueProcessor {
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

impl LanguageProcessor for VueProcessor {
    fn supported_extensions(&self) -> Vec<&'static str> {
        vec!["vue"]
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
                        && !path_str.starts_with("@/");

                    let dependency_type = if path_str == "vue" || path_str.starts_with("vue/") {
                        "vue_import"
                    } else if path_str.ends_with(".vue") {
                        "vue_component_import"
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
        if file_name == "App.vue" {
            return "vue_app".to_string();
        }

        if file_name == "index.vue" {
            return "vue_entry".to_string();
        }

        if file_name.to_lowercase().contains("page")
            || file_path.to_string_lossy().contains("/pages/")
            || file_path.to_string_lossy().contains("/views/")
        {
            return "vue_page".to_string();
        }

        if file_name.to_lowercase().contains("layout") {
            return "vue_layout".to_string();
        }

        // Check content patterns
        if content.contains("<template>") && content.contains("<script>") {
            if content.contains("export default") {
                "vue_component".to_string()
            } else {
                "vue_partial".to_string()
            }
        } else if content.contains("defineComponent") {
            "vue_composition_component".to_string()
        } else if content.contains("<script setup>") {
            "vue_setup_component".to_string()
        } else {
            "vue_file".to_string()
        }
    }

    fn is_important_line(&self, line: &str) -> bool {
        let trimmed = line.trim();

        // Vue template tags
        if trimmed.starts_with("<template>")
            || trimmed.starts_with("<script>")
            || trimmed.starts_with("<style>")
            || trimmed.starts_with("<script setup>")
        {
            return true;
        }

        // Vue component definitions
        if trimmed.contains("export default") || trimmed.contains("defineComponent") {
            return true;
        }

        // Vue Composition API
        if trimmed.contains("ref(")
            || trimmed.contains("reactive(")
            || trimmed.contains("computed(")
            || trimmed.contains("watch(")
            || trimmed.contains("onMounted")
            || trimmed.contains("onUnmounted")
        {
            return true;
        }

        // Import statements
        if trimmed.starts_with("import ") {
            return true;
        }

        // Vue directives and events
        if trimmed.contains("v-if")
            || trimmed.contains("v-for")
            || trimmed.contains("v-model")
            || trimmed.contains("@click")
            || trimmed.contains(":") && (trimmed.contains("=") || trimmed.contains("\""))
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
        "Vue"
    }

    fn extract_interfaces(&self, content: &str, _file_path: &Path) -> Vec<InterfaceInfo> {
        let mut interfaces = Vec::new();

        // Vue component interface analysis focuses on component definitions and methods
        if content.contains("<script") {
            // Extract Vue component name (from file name or export default)
            if content.contains("export default") {
                interfaces.push(InterfaceInfo {
                    name: "VueComponent".to_string(),
                    interface_type: "vue_component".to_string(),
                    visibility: "public".to_string(),
                    parameters: Vec::new(),
                    return_type: None,
                    description: Some("Vue single file component".to_string()),
                });
            }

            // Extract methods in methods block
            if let Some(methods_start) = content.find("methods:") {
                let methods_section = &content[methods_start..];
                for line in methods_section.lines().take(50) {
                    // Limit search range
                    let trimmed = line.trim();
                    if let Some(method_name) = self.extract_vue_method(trimmed) {
                        interfaces.push(InterfaceInfo {
                            name: method_name,
                            interface_type: "vue_method".to_string(),
                            visibility: "public".to_string(),
                            parameters: Vec::new(),
                            return_type: None,
                            description: None,
                        });
                    }
                }
            }
        }

        interfaces
    }
}

impl VueProcessor {
    /// Extract Vue method name
    fn extract_vue_method(&self, line: &str) -> Option<String> {
        // Match: methodName() { or methodName: function() {
        if line.contains('(') && line.contains('{') {
            if let Some(paren_pos) = line.find('(') {
                let before_paren = &line[..paren_pos].trim();
                if let Some(colon_pos) = before_paren.rfind(':') {
                    let method_name = before_paren[colon_pos + 1..].trim();
                    if !method_name.is_empty() && method_name != "function" {
                        return Some(method_name.to_string());
                    }
                } else if let Some(space_pos) = before_paren.rfind(' ') {
                    let method_name = before_paren[space_pos + 1..].trim();
                    if !method_name.is_empty() {
                        return Some(method_name.to_string());
                    }
                } else if !before_paren.is_empty() {
                    return Some(before_paren.to_string());
                }
            }
        }
        None
    }
}
