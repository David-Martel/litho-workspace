use super::{Dependency, LanguageProcessor};
use crate::types::code::{InterfaceInfo, ParameterInfo};
use regex::Regex;
use std::path::Path;

#[derive(Debug)]
pub struct PythonProcessor {
    import_regex: Regex,
    from_import_regex: Regex,
    function_regex: Regex,
    class_regex: Regex,
    method_regex: Regex,
    async_function_regex: Regex,
}

impl PythonProcessor {
    pub fn new() -> Self {
        Self {
            import_regex: Regex::new(r"^\s*import\s+([^\s#]+)").unwrap(),
            from_import_regex: Regex::new(r"^\s*from\s+([^\s]+)\s+import").unwrap(),
            function_regex: Regex::new(r"^\s*def\s+(\w+)\s*\(([^)]*)\)\s*(?:->\s*([^:]+))?:").unwrap(),
            class_regex: Regex::new(r"^\s*class\s+(\w+)(?:\([^)]*\))?:").unwrap(),
            method_regex: Regex::new(r"^\s+def\s+(\w+)\s*\(([^)]*)\)\s*(?:->\s*([^:]+))?:").unwrap(),
            async_function_regex: Regex::new(r"^\s*async\s+def\s+(\w+)\s*\(([^)]*)\)\s*(?:->\s*([^:]+))?:").unwrap(),
        }
    }
}

impl LanguageProcessor for PythonProcessor {
    fn supported_extensions(&self) -> Vec<&'static str> {
        vec!["py"]
    }
    
    fn extract_dependencies(&self, content: &str, file_path: &Path) -> Vec<Dependency> {
        let mut dependencies = Vec::new();
        let source_file = file_path.to_string_lossy().to_string();
        
        for (line_num, line) in content.lines().enumerate() {
            // Extract from...import statements
            if let Some(captures) = self.from_import_regex.captures(line) {
                if let Some(module_path) = captures.get(1) {
                    let module_str = module_path.as_str();
                    let is_external = !module_str.starts_with('.') && 
                                    !module_str.starts_with("__");
                    
                    dependencies.push(Dependency {
                        name: source_file.clone(),
                        path: Some(module_str.to_string()),
                        is_external,
                        line_number: Some(line_num + 1),
                        dependency_type: "from_import".to_string(),
                        version: None,
                    });
                }
            }
            // Extract import statements
            else if let Some(captures) = self.import_regex.captures(line) {
                if let Some(import_path) = captures.get(1) {
                    let import_str = import_path.as_str();
                    let is_external = !import_str.starts_with('.') && 
                                    !import_str.starts_with("__");
                    
                    dependencies.push(Dependency {
                        name: source_file.clone(),
                        path: Some(import_str.to_string()),
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
        let file_name = file_path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        
        if file_name == "__init__.py" {
            return "python_package".to_string();
        }
        
        if file_name == "main.py" || file_name == "app.py" {
            return "python_main".to_string();
        }
        
        if file_name.starts_with("test_") || file_name.ends_with("_test.py") {
            return "python_test".to_string();
        }
        
        if content.contains("class ") && content.contains("def __init__") {
            "python_class".to_string()
        } else if content.contains("def ") {
            "python_module".to_string()
        } else {
            "python_script".to_string()
        }
    }
    
    fn is_important_line(&self, line: &str) -> bool {
        let trimmed = line.trim();
        
        if trimmed.starts_with("class ") || trimmed.starts_with("def ") ||
           trimmed.starts_with("async def ") || trimmed.starts_with("import ") ||
           trimmed.starts_with("from ") {
            return true;
        }
        
        if trimmed.contains("TODO") || trimmed.contains("FIXME") || 
           trimmed.contains("NOTE") || trimmed.contains("HACK") {
            return true;
        }
        
        false
    }
    
    fn language_name(&self) -> &'static str {
        "Python"
    }

    fn extract_interfaces(&self, content: &str, _file_path: &Path) -> Vec<InterfaceInfo> {
        let mut interfaces = Vec::new();
        let lines: Vec<&str> = content.lines().collect();
        
        for (i, line) in lines.iter().enumerate() {
            // Extract async function definitions
            if let Some(captures) = self.async_function_regex.captures(line) {
                let name = captures.get(1).map(|m| m.as_str()).unwrap_or("").to_string();
                let params_str = captures.get(2).map(|m| m.as_str()).unwrap_or("");
                let return_type = captures.get(3).map(|m| m.as_str().trim().to_string());
                
                let parameters = self.parse_python_parameters(params_str);
                
                interfaces.push(InterfaceInfo {
                    name,
                    interface_type: "async_function".to_string(),
                    visibility: "public".to_string(),
                    parameters,
                    return_type,
                    description: self.extract_docstring(&lines, i),
                });
            }
            // Extract regular function definitions
            else if let Some(captures) = self.function_regex.captures(line) {
                let name = captures.get(1).map(|m| m.as_str()).unwrap_or("").to_string();
                let params_str = captures.get(2).map(|m| m.as_str()).unwrap_or("");
                let return_type = captures.get(3).map(|m| m.as_str().trim().to_string());
                
                let parameters = self.parse_python_parameters(params_str);
                
                interfaces.push(InterfaceInfo {
                    name,
                    interface_type: "function".to_string(),
                    visibility: "public".to_string(),
                    parameters,
                    return_type,
                    description: self.extract_docstring(&lines, i),
                });
            }
            
            // Extract class definitions
            if let Some(captures) = self.class_regex.captures(line) {
                let name = captures.get(1).map(|m| m.as_str()).unwrap_or("").to_string();
                
                interfaces.push(InterfaceInfo {
                    name,
                    interface_type: "class".to_string(),
                    visibility: "public".to_string(),
                    parameters: Vec::new(),
                    return_type: None,
                    description: self.extract_docstring(&lines, i),
                });
            }
            
            // Extract method definitions (inside classes)
            if let Some(captures) = self.method_regex.captures(line) {
                let name = captures.get(1).map(|m| m.as_str()).unwrap_or("").to_string();
                let params_str = captures.get(2).map(|m| m.as_str()).unwrap_or("");
                let return_type = captures.get(3).map(|m| m.as_str().trim().to_string());
                
                let parameters = self.parse_python_parameters(params_str);
                let visibility = if name.starts_with('_') {
                    if name.starts_with("__") && name.ends_with("__") {
                        "special"
                    } else {
                        "private"
                    }
                } else {
                    "public"
                };
                
                interfaces.push(InterfaceInfo {
                    name,
                    interface_type: "method".to_string(),
                    visibility: visibility.to_string(),
                    parameters,
                    return_type,
                    description: self.extract_docstring(&lines, i),
                });
            }
        }
        
        interfaces
    }
}

impl PythonProcessor {
    /// Parse Python function parameters
    fn parse_python_parameters(&self, params_str: &str) -> Vec<ParameterInfo> {
        let mut parameters = Vec::new();
        
        if params_str.trim().is_empty() {
            return parameters;
        }
        
        // Simple parameter parsing, handling basic cases
        for param in params_str.split(',') {
            let param = param.trim();
            if param.is_empty() || param == "self" || param == "cls" {
                continue;
            }
            
            // Parse parameter format: name, name: type, name = default, name: type = default
            let is_optional = param.contains('=');
            let mut param_type = "Any".to_string();
            let mut name = param.to_string();
            
            // Handle type annotations
            if let Some(colon_pos) = param.find(':') {
                name = param[..colon_pos].trim().to_string();
                let type_part = param[colon_pos + 1..].trim();
                
                if let Some(eq_pos) = type_part.find('=') {
                    param_type = type_part[..eq_pos].trim().to_string();
                } else {
                    param_type = type_part.to_string();
                }
            } else if let Some(eq_pos) = param.find('=') {
                name = param[..eq_pos].trim().to_string();
            }
            
            // Handle special parameters
            if name.starts_with('*') {
                if name.starts_with("**") {
                    name = name.trim_start_matches("**").to_string();
                    param_type = "dict".to_string();
                } else {
                    name = name.trim_start_matches('*').to_string();
                    param_type = "tuple".to_string();
                }
            }
            
            parameters.push(ParameterInfo {
                name,
                param_type,
                is_optional,
                description: None,
            });
        }
        
        parameters
    }
    
    /// Extract Python docstrings
    fn extract_docstring(&self, lines: &[&str], current_line: usize) -> Option<String> {
        // Find docstring after function/class definition
        if current_line + 1 < lines.len() {
            let next_line = lines[current_line + 1].trim();
            
            // Single-line docstring
            if (next_line.starts_with("\"\"\"") && next_line.ends_with("\"\"\"") && next_line.len() > 6) ||
               (next_line.starts_with("'''") && next_line.ends_with("'''") && next_line.len() > 6) {
                let content = if next_line.starts_with("\"\"\"") {
                    next_line.trim_start_matches("\"\"\"").trim_end_matches("\"\"\"").trim()
                } else {
                    next_line.trim_start_matches("'''").trim_end_matches("'''").trim()
                };
                return Some(content.to_string());
            }
            
            // Multi-line docstring
            if next_line.starts_with("\"\"\"") || next_line.starts_with("'''") {
                let quote_type = if next_line.starts_with("\"\"\"") { "\"\"\"" } else { "'''" };
                let mut doc_lines = Vec::new();
                
                // First line may contain content
                let first_content = next_line.trim_start_matches(quote_type).trim();
                if !first_content.is_empty() && !first_content.ends_with(quote_type) {
                    doc_lines.push(first_content.to_string());
                }
                
                // Find ending marker
                for i in (current_line + 2)..lines.len() {
                    let line = lines[i].trim();
                    if line.ends_with(quote_type) {
                        let content = line.trim_end_matches(quote_type).trim();
                        if !content.is_empty() {
                            doc_lines.push(content.to_string());
                        }
                        break;
                    } else if !line.is_empty() {
                        doc_lines.push(line.to_string());
                    }
                }
                
                if !doc_lines.is_empty() {
                    return Some(doc_lines.join(" "));
                }
            }
        }
        
        None
    }
}