use super::{Dependency, LanguageProcessor};
use crate::types::code::{InterfaceInfo, ParameterInfo};
use regex::Regex;
use std::path::Path;

#[derive(Debug)]
pub struct JavaScriptProcessor {
    import_regex: Regex,
    require_regex: Regex,
    dynamic_import_regex: Regex,
    function_regex: Regex,
    arrow_function_regex: Regex,
    class_regex: Regex,
    method_regex: Regex,
    export_function_regex: Regex,
}

impl JavaScriptProcessor {
    pub fn new() -> Self {
        Self {
            import_regex: Regex::new(r#"^\s*import\s+(?:.*\s+from\s+)?['"]([^'"]+)['"]"#).unwrap(),
            require_regex: Regex::new(r#"require\s*\(\s*['"]([^'"]+)['"]\s*\)"#).unwrap(),
            dynamic_import_regex: Regex::new(r#"import\s*\(\s*['"]([^'"]+)['"]\s*\)"#).unwrap(),
            function_regex: Regex::new(r"^\s*(async\s+)?function\s+(\w+)\s*\(([^)]*)\)").unwrap(),
            arrow_function_regex: Regex::new(
                r"^\s*(const|let|var)\s+(\w+)\s*=\s*(async\s+)?\(([^)]*)\)\s*=>",
            )
            .unwrap(),
            class_regex: Regex::new(r"^\s*class\s+(\w+)").unwrap(),
            method_regex: Regex::new(r"^\s*(async\s+)?(\w+)\s*\(([^)]*)\)\s*\{").unwrap(),
            export_function_regex: Regex::new(
                r"^\s*export\s+(async\s+)?function\s+(\w+)\s*\(([^)]*)\)",
            )
            .unwrap(),
        }
    }
}

impl LanguageProcessor for JavaScriptProcessor {
    fn supported_extensions(&self) -> Vec<&'static str> {
        vec!["js", "mjs", "cjs"]
    }

    fn extract_dependencies(&self, content: &str, file_path: &Path) -> Vec<Dependency> {
        let mut dependencies = Vec::new();
        let source_file = file_path.to_string_lossy().to_string();

        for (line_num, line) in content.lines().enumerate() {
            // Extract import statements
            if let Some(captures) = self.import_regex.captures(line) {
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

            // Extract require statements
            if let Some(captures) = self.require_regex.captures(line) {
                if let Some(require_path) = captures.get(1) {
                    let path_str = require_path.as_str();
                    let is_external = !path_str.starts_with('.') && !path_str.starts_with('/');

                    dependencies.push(Dependency {
                        name: source_file.clone(),
                        path: Some(path_str.to_string()),
                        is_external,
                        line_number: Some(line_num + 1),
                        dependency_type: "require".to_string(),
                        version: None,
                    });
                }
            }

            // Extract dynamic imports
            if let Some(captures) = self.dynamic_import_regex.captures(line) {
                if let Some(import_path) = captures.get(1) {
                    let path_str = import_path.as_str();
                    let is_external = !path_str.starts_with('.') && !path_str.starts_with('/');

                    dependencies.push(Dependency {
                        name: source_file.clone(),
                        path: Some(path_str.to_string()),
                        is_external,
                        line_number: Some(line_num + 1),
                        dependency_type: "dynamic_import".to_string(),
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
        if file_name == "index.js" || file_name == "main.js" || file_name == "app.js" {
            return "js_main".to_string();
        }

        if file_name.ends_with(".config.js") || file_name.ends_with(".conf.js") {
            return "js_config".to_string();
        }

        // Check file name suffix
        if file_name.ends_with(".test.js") || file_name.ends_with(".spec.js") {
            return "js_test".to_string();
        }

        // Check if path components contain test-related directories (to avoid misidentification)
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
            return "js_test".to_string();
        }

        // Check content patterns
        if content.contains("module.exports") || content.contains("exports.") {
            "js_module".to_string()
        } else if content.contains("export default") || content.contains("export {") {
            "js_es_module".to_string()
        } else if content.contains("function ")
            || content.contains("const ")
            || content.contains("let ")
        {
            "js_utility".to_string()
        } else {
            "js_file".to_string()
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

        // Class definitions
        if trimmed.starts_with("class ") {
            return true;
        }

        // Import/export statements
        if trimmed.starts_with("import ")
            || trimmed.starts_with("export ")
            || trimmed.starts_with("module.exports")
            || trimmed.contains("require(")
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
        "JavaScript"
    }

    fn extract_interfaces(&self, content: &str, _file_path: &Path) -> Vec<InterfaceInfo> {
        let mut interfaces = Vec::new();
        let lines: Vec<&str> = content.lines().collect();

        for (i, line) in lines.iter().enumerate() {
            // Extract exported function definitions
            if let Some(captures) = self.export_function_regex.captures(line) {
                let is_async = captures.get(1).is_some();
                let name = captures
                    .get(2)
                    .map(|m| m.as_str())
                    .unwrap_or("")
                    .to_string();
                let params_str = captures.get(3).map(|m| m.as_str()).unwrap_or("");

                let parameters = self.parse_javascript_parameters(params_str);
                let interface_type = if is_async {
                    "async_function"
                } else {
                    "function"
                };

                interfaces.push(InterfaceInfo {
                    name,
                    interface_type: interface_type.to_string(),
                    visibility: "public".to_string(),
                    parameters,
                    return_type: None,
                    description: self.extract_jsdoc_comment(&lines, i),
                });
            }
            // Extract regular function definitions
            else if let Some(captures) = self.function_regex.captures(line) {
                let is_async = captures.get(1).is_some();
                let name = captures
                    .get(2)
                    .map(|m| m.as_str())
                    .unwrap_or("")
                    .to_string();
                let params_str = captures.get(3).map(|m| m.as_str()).unwrap_or("");

                let parameters = self.parse_javascript_parameters(params_str);
                let interface_type = if is_async {
                    "async_function"
                } else {
                    "function"
                };

                interfaces.push(InterfaceInfo {
                    name,
                    interface_type: interface_type.to_string(),
                    visibility: "private".to_string(),
                    parameters,
                    return_type: None,
                    description: self.extract_jsdoc_comment(&lines, i),
                });
            }

            // Extract arrow function definitions
            if let Some(captures) = self.arrow_function_regex.captures(line) {
                let _var_type = captures.get(1).map(|m| m.as_str()).unwrap_or("");
                let name = captures
                    .get(2)
                    .map(|m| m.as_str())
                    .unwrap_or("")
                    .to_string();
                let is_async = captures.get(3).is_some();
                let params_str = captures.get(4).map(|m| m.as_str()).unwrap_or("");

                let parameters = self.parse_javascript_parameters(params_str);
                let interface_type = if is_async {
                    "async_arrow_function"
                } else {
                    "arrow_function"
                };

                interfaces.push(InterfaceInfo {
                    name,
                    interface_type: interface_type.to_string(),
                    visibility: "private".to_string(),
                    parameters,
                    return_type: None,
                    description: self.extract_jsdoc_comment(&lines, i),
                });
            }

            // Extract class definitions
            if let Some(captures) = self.class_regex.captures(line) {
                let name = captures
                    .get(1)
                    .map(|m| m.as_str())
                    .unwrap_or("")
                    .to_string();

                interfaces.push(InterfaceInfo {
                    name,
                    interface_type: "class".to_string(),
                    visibility: "public".to_string(),
                    parameters: Vec::new(),
                    return_type: None,
                    description: self.extract_jsdoc_comment(&lines, i),
                });
            }

            // Extract method definitions (inside classes)
            if let Some(captures) = self.method_regex.captures(line) {
                let is_async = captures.get(1).is_some();
                let name = captures
                    .get(2)
                    .map(|m| m.as_str())
                    .unwrap_or("")
                    .to_string();
                let params_str = captures.get(3).map(|m| m.as_str()).unwrap_or("");

                // Skip some common non-method patterns
                if name == "if" || name == "for" || name == "while" || name == "switch" {
                    continue;
                }

                let parameters = self.parse_javascript_parameters(params_str);
                let interface_type = if is_async { "async_method" } else { "method" };

                interfaces.push(InterfaceInfo {
                    name,
                    interface_type: interface_type.to_string(),
                    visibility: "public".to_string(),
                    parameters,
                    return_type: None,
                    description: self.extract_jsdoc_comment(&lines, i),
                });
            }
        }

        interfaces
    }
}

impl JavaScriptProcessor {
    /// Parse JavaScript function parameters
    fn parse_javascript_parameters(&self, params_str: &str) -> Vec<ParameterInfo> {
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

            // Handle default parameters
            let is_optional = param.contains('=');
            let name = if let Some(eq_pos) = param.find('=') {
                param[..eq_pos].trim().to_string()
            } else {
                param.to_string()
            };

            // Handle destructured parameters
            let clean_name = if name.starts_with('{') && name.ends_with('}') {
                format!("destructured_{}", parameters.len())
            } else if name.starts_with('[') && name.ends_with(']') {
                format!("array_destructured_{}", parameters.len())
            } else {
                name
            };

            parameters.push(ParameterInfo {
                name: clean_name,
                param_type: "any".to_string(), // JavaScript doesn't have static types
                is_optional,
                description: None,
            });
        }

        parameters
    }

    /// Extract JSDoc comments
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
