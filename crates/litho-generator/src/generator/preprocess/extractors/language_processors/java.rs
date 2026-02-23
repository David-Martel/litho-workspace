use super::{Dependency, LanguageProcessor};
use crate::types::code::{InterfaceInfo, ParameterInfo};
use regex::Regex;
use std::path::Path;

#[derive(Debug)]
pub struct JavaProcessor {
    import_regex: Regex,
    package_regex: Regex,
    method_regex: Regex,
    class_regex: Regex,
    interface_regex: Regex,
    enum_regex: Regex,
    constructor_regex: Regex,
}

impl JavaProcessor {
    pub fn new() -> Self {
        Self {
            import_regex: Regex::new(r"^\s*import\s+([^;]+);").unwrap(),
            package_regex: Regex::new(r"^\s*package\s+([^;]+);").unwrap(),
            method_regex: Regex::new(r"^\s*(public|private|protected)?\s*(static)?\s*(final)?\s*(\w+)\s+(\w+)\s*\(([^)]*)\)").unwrap(),
            class_regex: Regex::new(r"^\s*(public|private|protected)?\s*(abstract)?\s*(final)?\s*class\s+(\w+)").unwrap(),
            interface_regex: Regex::new(r"^\s*(public|private|protected)?\s*interface\s+(\w+)").unwrap(),
            enum_regex: Regex::new(r"^\s*(public|private|protected)?\s*enum\s+(\w+)").unwrap(),
            constructor_regex: Regex::new(r"^\s*(public|private|protected)?\s*(\w+)\s*\(([^)]*)\)").unwrap(),
        }
    }
}

impl LanguageProcessor for JavaProcessor {
    fn supported_extensions(&self) -> Vec<&'static str> {
        vec!["java"]
    }
    
    fn extract_dependencies(&self, content: &str, file_path: &Path) -> Vec<Dependency> {
        let mut dependencies = Vec::new();
        let source_file = file_path.to_string_lossy().to_string();
        
        for (line_num, line) in content.lines().enumerate() {
            // Extract import statements
            if let Some(captures) = self.import_regex.captures(line) {
                if let Some(import_path) = captures.get(1) {
                    let import_str = import_path.as_str().trim();
                    let is_external = import_str.starts_with("java.") || 
                                    import_str.starts_with("javax.") ||
                                    !import_str.contains(".");
                    
                    // Parse dependency name
                    let dependency_name = self.extract_dependency_name(import_str);
                    
                    dependencies.push(Dependency {
                        name: dependency_name,
                        path: Some(source_file.clone()),
                        is_external,
                        line_number: Some(line_num + 1),
                        dependency_type: "import".to_string(),
                        version: None,
                    });
                }
            }
            
            // Extract package statement
            if let Some(captures) = self.package_regex.captures(line) {
                if let Some(package_name) = captures.get(1) {
                    dependencies.push(Dependency {
                        name: package_name.as_str().trim().to_string(),
                        path: Some(source_file.clone()),
                        is_external: false,
                        line_number: Some(line_num + 1),
                        dependency_type: "package".to_string(),
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
        
        if file_name.ends_with("Test.java") || file_name.ends_with("Tests.java") {
            return "java_test".to_string();
        }
        
        if content.contains("interface ") {
            "java_interface".to_string()
        } else if content.contains("enum ") {
            "java_enum".to_string()
        } else if content.contains("abstract class") {
            "java_abstract_class".to_string()
        } else if content.contains("class ") {
            "java_class".to_string()
        } else {
            "java_file".to_string()
        }
    }
    
    fn is_important_line(&self, line: &str) -> bool {
        let trimmed = line.trim();
        
        if trimmed.starts_with("public class ") || trimmed.starts_with("class ") ||
           trimmed.starts_with("interface ") || trimmed.starts_with("enum ") ||
           trimmed.starts_with("public ") || trimmed.starts_with("private ") ||
           trimmed.starts_with("protected ") || trimmed.starts_with("import ") ||
           trimmed.starts_with("package ") {
            return true;
        }
        
        if trimmed.contains("TODO") || trimmed.contains("FIXME") || 
           trimmed.contains("NOTE") || trimmed.contains("HACK") {
            return true;
        }
        
        false
    }
    
    fn language_name(&self) -> &'static str {
        "Java"
    }

    fn extract_interfaces(&self, content: &str, _file_path: &Path) -> Vec<InterfaceInfo> {
        let mut interfaces = Vec::new();
        let lines: Vec<&str> = content.lines().collect();
        
        for (i, line) in lines.iter().enumerate() {
            // Extract class definitions
            if let Some(captures) = self.class_regex.captures(line) {
                let visibility = captures.get(1).map(|m| m.as_str()).unwrap_or("package");
                let is_abstract = captures.get(2).is_some();
                let is_final = captures.get(3).is_some();
                let name = captures.get(4).map(|m| m.as_str()).unwrap_or("").to_string();
                
                let mut interface_type = "class".to_string();
                if is_abstract {
                    interface_type = "abstract_class".to_string();
                } else if is_final {
                    interface_type = "final_class".to_string();
                }
                
                interfaces.push(InterfaceInfo {
                    name,
                    interface_type,
                    visibility: visibility.to_string(),
                    parameters: Vec::new(),
                    return_type: None,
                    description: self.extract_javadoc(&lines, i),
                });
            }
            
            // Extract interface definitions
            if let Some(captures) = self.interface_regex.captures(line) {
                let visibility = captures.get(1).map(|m| m.as_str()).unwrap_or("package");
                let name = captures.get(2).map(|m| m.as_str()).unwrap_or("").to_string();
                
                interfaces.push(InterfaceInfo {
                    name,
                    interface_type: "interface".to_string(),
                    visibility: visibility.to_string(),
                    parameters: Vec::new(),
                    return_type: None,
                    description: self.extract_javadoc(&lines, i),
                });
            }
            
            // Extract enum definitions
            if let Some(captures) = self.enum_regex.captures(line) {
                let visibility = captures.get(1).map(|m| m.as_str()).unwrap_or("package");
                let name = captures.get(2).map(|m| m.as_str()).unwrap_or("").to_string();
                
                interfaces.push(InterfaceInfo {
                    name,
                    interface_type: "enum".to_string(),
                    visibility: visibility.to_string(),
                    parameters: Vec::new(),
                    return_type: None,
                    description: self.extract_javadoc(&lines, i),
                });
            }
            
            // Extract method definitions
            if let Some(captures) = self.method_regex.captures(line) {
                let visibility = captures.get(1).map(|m| m.as_str()).unwrap_or("package");
                let is_static = captures.get(2).is_some();
                let is_final = captures.get(3).is_some();
                let return_type = captures.get(4).map(|m| m.as_str()).unwrap_or("").to_string();
                let name = captures.get(5).map(|m| m.as_str()).unwrap_or("").to_string();
                let params_str = captures.get(6).map(|m| m.as_str()).unwrap_or("");
                
                // Skip some Java keywords
                if return_type == "if" || return_type == "for" || return_type == "while" || 
                   return_type == "switch" || return_type == "try" {
                    continue;
                }
                
                let parameters = self.parse_java_parameters(params_str);
                let mut interface_type = "method".to_string();
                if is_static {
                    interface_type = "static_method".to_string();
                } else if is_final {
                    interface_type = "final_method".to_string();
                }
                
                interfaces.push(InterfaceInfo {
                    name,
                    interface_type,
                    visibility: visibility.to_string(),
                    parameters,
                    return_type: Some(return_type),
                    description: self.extract_javadoc(&lines, i),
                });
            }
            
            // Extract constructors
            if let Some(captures) = self.constructor_regex.captures(line) {
                let visibility = captures.get(1).map(|m| m.as_str()).unwrap_or("package");
                let name = captures.get(2).map(|m| m.as_str()).unwrap_or("").to_string();
                let params_str = captures.get(3).map(|m| m.as_str()).unwrap_or("");
                
                // Simple check if it's a constructor (name starts with uppercase)
                if name.chars().next().map_or(false, |c| c.is_uppercase()) {
                    let parameters = self.parse_java_parameters(params_str);
                    
                    interfaces.push(InterfaceInfo {
                        name,
                        interface_type: "constructor".to_string(),
                        visibility: visibility.to_string(),
                        parameters,
                        return_type: None,
                        description: self.extract_javadoc(&lines, i),
                    });
                }
            }
        }
        
        interfaces
    }
}

impl JavaProcessor {
    /// Parse Java method parameters
    fn parse_java_parameters(&self, params_str: &str) -> Vec<ParameterInfo> {
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
            
            // Parse parameter format: Type name or final Type name
            let parts: Vec<&str> = param.split_whitespace().collect();
            if parts.len() >= 2 {
                let (param_type, name) = if parts[0] == "final" && parts.len() >= 3 {
                    (parts[1].to_string(), parts[2].to_string())
                } else {
                    (parts[0].to_string(), parts[1].to_string())
                };
                
                // Handle generic types
                let clean_type = if param_type.contains('<') {
                    param_type
                } else {
                    param_type
                };
                
                parameters.push(ParameterInfo {
                    name,
                    param_type: clean_type,
                    is_optional: false, // Java doesn't have optional parameters
                    description: None,
                });
            }
        }
        
        parameters
    }
    
    /// Extract Javadoc comments
    fn extract_javadoc(&self, lines: &[&str], current_line: usize) -> Option<String> {
        let mut doc_lines = Vec::new();
        let mut in_javadoc = false;
        
        // Search upward for Javadoc comments
        for i in (0..current_line).rev() {
            let line = lines[i].trim();
            
            if line.ends_with("*/") {
                in_javadoc = true;
                if line.starts_with("/**") {
                    // Single-line Javadoc
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
            } else if in_javadoc {
                if line.starts_with("/**") {
                    let content = line.trim_start_matches("/**").trim();
                    if !content.is_empty() && content != "*" {
                        doc_lines.insert(0, content.to_string());
                    }
                    break;
                } else if line.starts_with('*') {
                    let content = line.trim_start_matches('*').trim();
                    if !content.is_empty() && !content.starts_with('@') {
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

    /// Extract dependency name from Java import path
    fn extract_dependency_name(&self, import_path: &str) -> String {
        // For com.example.package.ClassName, return ClassName
        if let Some(class_name) = import_path.split('.').last() {
            class_name.to_string()
        } else {
            import_path.to_string()
        }
    }
}