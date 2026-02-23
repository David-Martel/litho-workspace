use super::{Dependency, LanguageProcessor};
use crate::types::code::{InterfaceInfo, ParameterInfo};
use regex::Regex;
use std::path::Path;

#[derive(Debug)]
pub struct TypeScriptProcessor {
    import_regex: Regex,
    type_import_regex: Regex,
    function_regex: Regex,
    interface_regex: Regex,
    type_alias_regex: Regex,
    class_regex: Regex,
    enum_regex: Regex,
    method_regex: Regex,
}

impl TypeScriptProcessor {
    pub fn new() -> Self {
        Self {
            import_regex: Regex::new(r#"^\s*import\s+(?:.*\s+from\s+)?['"]([^'"]+)['"]"#).unwrap(),
            type_import_regex: Regex::new(r#"^\s*import\s+type\s+.*\s+from\s+['"]([^'"]+)['"]"#).unwrap(),
            function_regex: Regex::new(r"^\s*(export\s+)?(async\s+)?function\s+(\w+)\s*\(([^)]*)\)\s*:\s*([^{]+)?").unwrap(),
            interface_regex: Regex::new(r"^\s*(export\s+)?interface\s+(\w+)").unwrap(),
            type_alias_regex: Regex::new(r"^\s*(export\s+)?type\s+(\w+)\s*=").unwrap(),
            class_regex: Regex::new(r"^\s*(export\s+)?(abstract\s+)?class\s+(\w+)").unwrap(),
            enum_regex: Regex::new(r"^\s*(export\s+)?enum\s+(\w+)").unwrap(),
            method_regex: Regex::new(r"^\s*(public|private|protected)?\s*(static\s+)?(async\s+)?(\w+)\s*\(([^)]*)\)\s*:\s*([^{]+)?").unwrap(),
        }
    }
}

impl LanguageProcessor for TypeScriptProcessor {
    fn supported_extensions(&self) -> Vec<&'static str> {
        vec!["ts", "tsx"]
    }

    fn extract_dependencies(&self, content: &str, file_path: &Path) -> Vec<Dependency> {
        let mut dependencies = Vec::new();
        let source_file = file_path.to_string_lossy().to_string();

        for (line_num, line) in content.lines().enumerate() {
            // Extract type import statements
            if let Some(captures) = self.type_import_regex.captures(line) {
                if let Some(import_path) = captures.get(1) {
                    let path_str = import_path.as_str();
                    let is_external = !path_str.starts_with('.') && !path_str.starts_with('/');

                    dependencies.push(Dependency {
                        name: source_file.clone(),
                        path: Some(path_str.to_string()),
                        is_external,
                        line_number: Some(line_num + 1),
                        dependency_type: "type_import".to_string(),
                        version: None,
                    });
                }
            }
            // Extract regular import statements
            else if let Some(captures) = self.import_regex.captures(line) {
                if let Some(import_path) = captures.get(1) {
                    let path_str = import_path.as_str();
                    let is_external = !path_str.starts_with('.') && !path_str.starts_with('/');

                    dependencies.push(Dependency {
                        name: source_file.clone(),
                        path: Some(path_str.to_string()),
                        is_external,
                        line_number: Some(line_num + 1),
                        dependency_type: "import".to_string(),
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
        if file_name == "index.ts" || file_name == "main.ts" || file_name == "app.ts" {
            return "ts_main".to_string();
        }

        if file_name.ends_with(".d.ts") {
            return "ts_declaration".to_string();
        }

        if file_name.ends_with(".config.ts") || file_name.ends_with(".conf.ts") {
            return "ts_config".to_string();
        }

        // Check file name suffix
        if file_name.ends_with(".test.ts") || file_name.ends_with(".spec.ts") {
            return "ts_test".to_string();
        }

        // Check if path components contain test-related directories (avoid false positives)
        let has_test_dir = file_path.components().any(|component| {
            if let std::path::Component::Normal(name) = component {
                let name_str = name.to_string_lossy().to_lowercase();
                name_str == "tests"
                    || name_str == "test"
                    || name_str == "spec"
                    || name_str == "__tests__"
                    || name_str == "__spec__"
            } else {
                false
            }
        });

        if has_test_dir {
            return "ts_test".to_string();
        }

        // Check content patterns
        if content.contains("interface ") || content.contains("type ") {
            "ts_types".to_string()
        } else if content.contains("class ") && content.contains("extends") {
            "ts_class".to_string()
        } else if content.contains("enum ") {
            "ts_enum".to_string()
        } else if content.contains("namespace ") {
            "ts_namespace".to_string()
        } else if content.contains("export default") || content.contains("export {") {
            "ts_module".to_string()
        } else {
            "ts_file".to_string()
        }
    }

    fn is_important_line(&self, line: &str) -> bool {
        let trimmed = line.trim();

        // Function definitions
        if trimmed.starts_with("function ")
            || trimmed.starts_with("async function ")
            || trimmed.contains("=> {")
            || trimmed.contains("= function")
        {
            return true;
        }

        // Class, interface, type definitions
        if trimmed.starts_with("class ")
            || trimmed.starts_with("interface ")
            || trimmed.starts_with("type ")
            || trimmed.starts_with("enum ")
        {
            return true;
        }

        // Import/export statements
        if trimmed.starts_with("import ") || trimmed.starts_with("export ") {
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
        "TypeScript"
    }

    fn extract_interfaces(&self, content: &str, _file_path: &Path) -> Vec<InterfaceInfo> {
        let mut interfaces = Vec::new();
        let lines: Vec<&str> = content.lines().collect();

        for (i, line) in lines.iter().enumerate() {
            // Extract function definitions
            if let Some(captures) = self.function_regex.captures(line) {
                let is_exported = captures.get(1).is_some();
                let is_async = captures.get(2).is_some();
                let name = captures
                    .get(3)
                    .map(|m| m.as_str())
                    .unwrap_or("")
                    .to_string();
                let params_str = captures.get(4).map(|m| m.as_str()).unwrap_or("");
                let return_type = captures.get(5).map(|m| m.as_str().trim().to_string());

                let parameters = self.parse_typescript_parameters(params_str);
                let visibility = if is_exported { "public" } else { "private" };
                let interface_type = if is_async {
                    "async_function"
                } else {
                    "function"
                };

                interfaces.push(InterfaceInfo {
                    name,
                    interface_type: interface_type.to_string(),
                    visibility: visibility.to_string(),
                    parameters,
                    return_type,
                    description: self.extract_jsdoc_comment(&lines, i),
                });
            }

            // Extract interface definitions
            if let Some(captures) = self.interface_regex.captures(line) {
                let is_exported = captures.get(1).is_some();
                let name = captures
                    .get(2)
                    .map(|m| m.as_str())
                    .unwrap_or("")
                    .to_string();
                let visibility = if is_exported { "public" } else { "private" };

                interfaces.push(InterfaceInfo {
                    name,
                    interface_type: "interface".to_string(),
                    visibility: visibility.to_string(),
                    parameters: Vec::new(),
                    return_type: None,
                    description: self.extract_jsdoc_comment(&lines, i),
                });
            }

            // Extract type alias definitions
            if let Some(captures) = self.type_alias_regex.captures(line) {
                let is_exported = captures.get(1).is_some();
                let name = captures
                    .get(2)
                    .map(|m| m.as_str())
                    .unwrap_or("")
                    .to_string();
                let visibility = if is_exported { "public" } else { "private" };

                interfaces.push(InterfaceInfo {
                    name,
                    interface_type: "type_alias".to_string(),
                    visibility: visibility.to_string(),
                    parameters: Vec::new(),
                    return_type: None,
                    description: self.extract_jsdoc_comment(&lines, i),
                });
            }

            // Extract class definitions
            if let Some(captures) = self.class_regex.captures(line) {
                let is_exported = captures.get(1).is_some();
                let is_abstract = captures.get(2).is_some();
                let name = captures
                    .get(3)
                    .map(|m| m.as_str())
                    .unwrap_or("")
                    .to_string();
                let visibility = if is_exported { "public" } else { "private" };
                let interface_type = if is_abstract {
                    "abstract_class"
                } else {
                    "class"
                };

                interfaces.push(InterfaceInfo {
                    name,
                    interface_type: interface_type.to_string(),
                    visibility: visibility.to_string(),
                    parameters: Vec::new(),
                    return_type: None,
                    description: self.extract_jsdoc_comment(&lines, i),
                });
            }

            // Extract enum definitions
            if let Some(captures) = self.enum_regex.captures(line) {
                let is_exported = captures.get(1).is_some();
                let name = captures
                    .get(2)
                    .map(|m| m.as_str())
                    .unwrap_or("")
                    .to_string();
                let visibility = if is_exported { "public" } else { "private" };

                interfaces.push(InterfaceInfo {
                    name,
                    interface_type: "enum".to_string(),
                    visibility: visibility.to_string(),
                    parameters: Vec::new(),
                    return_type: None,
                    description: self.extract_jsdoc_comment(&lines, i),
                });
            }

            // Extract method definitions (inside classes)
            if let Some(captures) = self.method_regex.captures(line) {
                let visibility = captures.get(1).map(|m| m.as_str()).unwrap_or("public");
                let is_static = captures.get(2).is_some();
                let is_async = captures.get(3).is_some();
                let name = captures
                    .get(4)
                    .map(|m| m.as_str())
                    .unwrap_or("")
                    .to_string();
                let params_str = captures.get(5).map(|m| m.as_str()).unwrap_or("");
                let return_type = captures.get(6).map(|m| m.as_str().trim().to_string());

                let parameters = self.parse_typescript_parameters(params_str);
                let mut interface_type = if is_async { "async_method" } else { "method" };
                if is_static {
                    interface_type = if is_async {
                        "static_async_method"
                    } else {
                        "static_method"
                    };
                }

                interfaces.push(InterfaceInfo {
                    name,
                    interface_type: interface_type.to_string(),
                    visibility: visibility.to_string(),
                    parameters,
                    return_type,
                    description: self.extract_jsdoc_comment(&lines, i),
                });
            }
        }

        interfaces
    }
}

impl TypeScriptProcessor {
    /// Parse TypeScript function parameters
    fn parse_typescript_parameters(&self, params_str: &str) -> Vec<ParameterInfo> {
        let mut parameters = Vec::new();

        if params_str.trim().is_empty() {
            return parameters;
        }

        // Simple parameter parsing, handling basic cases
        for param in params_str.split(',') {
            let param = param.trim();
            if param.is_empty() {
                continue;
            }

            // Parse parameter format: name: type or name?: type or name: type = default
            let is_optional = param.contains('?') || param.contains('=');

            if let Some(colon_pos) = param.find(':') {
                let name_part = param[..colon_pos].trim();
                let name = name_part.replace('?', "").trim().to_string();
                let type_part = param[colon_pos + 1..].trim();
                let param_type = if let Some(eq_pos) = type_part.find('=') {
                    type_part[..eq_pos].trim().to_string()
                } else {
                    type_part.to_string()
                };

                parameters.push(ParameterInfo {
                    name,
                    param_type,
                    is_optional,
                    description: None,
                });
            }
        }

        parameters
    }

    /// Extract JSDoc comment
    fn extract_jsdoc_comment(&self, lines: &[&str], current_line: usize) -> Option<String> {
        let mut doc_lines = Vec::new();
        let mut in_jsdoc = false;

        // Search upward for JSDoc comments
        for i in (0..current_line).rev() {
            let line = lines[i].trim();

            if line.ends_with("*/") {
                in_jsdoc = true;
                if line.starts_with("/**") {
                    // Single-line JSDoc
                    let content = line.trim_start_matches("/**").trim_end_matches("*/").trim();
                    if !content.is_empty() {
                        doc_lines.insert(0, content.to_string());
                    }
                    break;
                } else {
                    let content = line.trim_end_matches("*/").trim();
                    if !content.is_empty() && content != "*" {
                        doc_lines.insert(0, content.trim_start_matches('*').trim().to_string());
                    }
                }
            } else if in_jsdoc {
                if line.starts_with("/**") {
                    let content = line.trim_start_matches("/**").trim();
                    if !content.is_empty() && content != "*" {
                        doc_lines.insert(0, content.to_string());
                    }
                    break;
                } else if line.starts_with('*') {
                    let content = line.trim_start_matches('*').trim();
                    if !content.is_empty() {
                        doc_lines.insert(0, content.to_string());
                    }
                }
            } else if !line.is_empty() {
                break;
            }
        }

        if doc_lines.is_empty() {
            None
        } else {
            Some(doc_lines.join(" "))
        }
    }
}
