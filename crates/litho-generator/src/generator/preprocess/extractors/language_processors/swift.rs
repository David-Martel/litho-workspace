use super::{Dependency, LanguageProcessor};
use crate::types::code::{InterfaceInfo, ParameterInfo};
use regex::Regex;
use std::path::Path;

/// Swift language processor
#[derive(Debug)]
pub struct SwiftProcessor {
    import_regex: Regex,
    func_regex: Regex,
    init_regex: Regex,
}

impl SwiftProcessor {
    pub fn new() -> Self {
        Self {
            // import Foundation, @_exported import Module, @testable import Module
            import_regex: Regex::new(r"^\s*(?:@\w+\s+)*import\s+(\w+)").unwrap(),

            // func name(params) async throws -> ReturnType
            // Supports: @objc func, @available(...) public func, final func, etc.
            // Pattern: optional attributes (@xxx or @xxx(...)), then modifiers, then func
            func_regex: Regex::new(
                r"(?:@\w+(?:\([^)]*\))?\s+)*(?:(?:public|private|internal|fileprivate|open)\s+)?(?:final\s+)?(?:static\s+)?(?:class\s+)?(?:override\s+)?(?:mutating\s+)?func\s+(\w+)"
            ).unwrap(),

            // init(params), init?(params), init<T>(params)
            // Supports: @objc init, @available(...) public init, generic init<T>, etc.
            init_regex: Regex::new(
                r"(?:@\w+(?:\([^)]*\))?\s+)*(?:(?:public|private|internal|fileprivate|open)\s+)?(?:convenience\s+)?(?:required\s+)?(?:override\s+)?init\s*(?:<[^>]+>)?\s*(\?|!)?\s*\("
            ).unwrap(),
        }
    }

    fn extract_visibility(line: &str) -> String {
        if line.contains("fileprivate ") {
            "fileprivate".to_string()
        } else if line.contains("private ") {
            "private".to_string()
        } else if line.contains("public ") {
            "public".to_string()
        } else if line.contains("open ") {
            "open".to_string()
        } else {
            "internal".to_string()
        }
    }

    fn extract_name_after_keyword(line: &str, keyword: &str) -> Option<String> {
        if let Some(pos) = line.find(keyword) {
            let after = &line[pos + keyword.len()..];
            let name: String = after
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                return Some(name);
            }
        }
        None
    }

    fn extract_func_name(line: &str) -> Option<String> {
        if let Some(pos) = line.find("func ") {
            let after = &line[pos + 5..];
            // Handle generic functions: func name<T>
            let name: String = after
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                return Some(name);
            }
        }
        None
    }

    fn extract_return_type(line: &str) -> Option<String> {
        if let Some(arrow_pos) = line.find("->") {
            let after_arrow = &line[arrow_pos + 2..];
            let return_type: String = after_arrow
                .trim()
                .chars()
                .take_while(|c| *c != '{' && *c != '\n')
                .collect();
            let trimmed = return_type.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
        None
    }

    fn parse_parameters(params_str: &str) -> Vec<ParameterInfo> {
        let mut parameters = Vec::new();
        if params_str.trim().is_empty() {
            return parameters;
        }

        // Handle nested brackets in generic types
        let mut depth = 0;
        let mut current = String::new();
        let mut params = Vec::new();

        for ch in params_str.chars() {
            match ch {
                '<' | '(' | '[' => {
                    depth += 1;
                    current.push(ch);
                }
                '>' | ')' | ']' => {
                    depth -= 1;
                    current.push(ch);
                }
                ',' if depth == 0 => {
                    params.push(current.trim().to_string());
                    current.clear();
                }
                _ => current.push(ch),
            }
        }
        if !current.trim().is_empty() {
            params.push(current.trim().to_string());
        }

        for param in params {
            if let Some(colon_pos) = param.rfind(':') {
                let name_part = param[..colon_pos].trim();
                let type_part = param[colon_pos + 1..].trim();

                // Get actual parameter name (last word before colon)
                let name = name_part
                    .split_whitespace()
                    .last()
                    .unwrap_or(name_part)
                    .to_string();

                let is_optional = type_part.ends_with('?') || type_part.contains("Optional<");

                parameters.push(ParameterInfo {
                    name,
                    param_type: type_part.to_string(),
                    is_optional,
                    description: None,
                });
            }
        }

        parameters
    }

    fn extract_params_string(line: &str) -> String {
        if let Some(start) = line.find('(') {
            let mut depth = 0;
            let mut end = start;
            for (i, ch) in line[start..].char_indices() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = start + i;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            return line[start + 1..end].to_string();
        }
        String::new()
    }

    fn infer_type_from_value(line: &str) -> String {
        if let Some(eq_pos) = line.find('=') {
            let value_part = line[eq_pos + 1..].trim();

            // Check for common literal types
            if value_part.starts_with('"') || value_part.starts_with("\"\"\"") {
                return "String".to_string();
            }
            if value_part == "true" || value_part == "false" {
                return "Bool".to_string();
            }
            if value_part == "nil" {
                return "Optional".to_string();
            }
            if value_part.starts_with('[') && value_part.contains(']') {
                // Array or dictionary literal
                if value_part.contains(':') {
                    return "Dictionary".to_string();
                }
                return "Array".to_string();
            }
            // Check for numeric literals
            if value_part
                .chars()
                .next()
                .map(|c| c.is_ascii_digit() || c == '-')
                .unwrap_or(false)
            {
                if value_part.contains('.') {
                    return "Double".to_string();
                }
                return "Int".to_string();
            }

            // Check for constructor call: TypeName() or TypeName(args)
            // Extract the type name before the parenthesis
            if let Some(paren_pos) = value_part.find('(') {
                let type_name: String = value_part[..paren_pos]
                    .trim()
                    .chars()
                    .take_while(|c| {
                        c.is_alphanumeric() || *c == '_' || *c == '<' || *c == '>' || *c == '.'
                    })
                    .collect();
                if !type_name.is_empty() && type_name.chars().next().unwrap().is_uppercase() {
                    return type_name;
                }
            }

            // Check for static member access: .something
            if value_part.starts_with('.') {
                return "inferred".to_string();
            }
        }
        "inferred".to_string()
    }

    fn extract_doc_comment(lines: &[&str], current_line: usize) -> Option<String> {
        let mut doc_lines = Vec::new();
        let mut in_block = false;

        for i in (0..current_line).rev() {
            let line = lines[i].trim();

            // Single line doc comment
            if line.starts_with("///") {
                doc_lines.insert(0, line.trim_start_matches("///").trim().to_string());
                continue;
            }

            // Block comment handling
            if line.ends_with("*/") && !in_block {
                in_block = true;
                if line.starts_with("/**") {
                    // Single line block: /** comment */
                    let content = line.trim_start_matches("/**").trim_end_matches("*/").trim();
                    if !content.is_empty() {
                        doc_lines.insert(0, content.to_string());
                    }
                    break;
                }
                continue;
            }

            if in_block {
                if line.starts_with("/**") {
                    let content = line.trim_start_matches("/**").trim();
                    if !content.is_empty() {
                        doc_lines.insert(0, content.to_string());
                    }
                    break;
                } else if line.starts_with('*') {
                    let content = line.trim_start_matches('*').trim();
                    if !content.is_empty() && !content.starts_with('-') && !content.starts_with('@')
                    {
                        doc_lines.insert(0, content.to_string());
                    }
                }
                continue;
            }

            // Stop at non-empty, non-attribute lines
            if !line.is_empty() && !line.starts_with('@') {
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

impl Default for SwiftProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageProcessor for SwiftProcessor {
    fn supported_extensions(&self) -> Vec<&'static str> {
        vec!["swift"]
    }

    fn extract_dependencies(&self, content: &str, file_path: &Path) -> Vec<Dependency> {
        let mut dependencies = Vec::new();
        let source_file = file_path.to_string_lossy().to_string();

        for (line_num, line) in content.lines().enumerate() {
            if let Some(captures) = self.import_regex.captures(line) {
                if let Some(module) = captures.get(1) {
                    let name = module.as_str().to_string();
                    // In Swift, all imports are external dependencies because:
                    // - Code within the same target doesn't require import statements
                    // - Both system frameworks (Foundation, UIKit) and third-party
                    //   packages (Alamofire, etc.) require explicit imports
                    dependencies.push(Dependency {
                        name: name.clone(),
                        path: Some(source_file.clone()),
                        is_external: true,
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

        // Special file names
        if file_name == "AppDelegate.swift" {
            return "swift_app_delegate".to_string();
        }
        if file_name == "SceneDelegate.swift" {
            return "swift_scene_delegate".to_string();
        }
        if file_name.ends_with("ViewController.swift") {
            return "swift_view_controller".to_string();
        }
        if file_name.ends_with("Tests.swift") || file_name.ends_with("Test.swift") {
            return "swift_test".to_string();
        }

        // Content-based detection (first-match strategy, consistent with other language processors)
        if content.contains("@main") && content.contains(": App") {
            return "swift_swiftui_app".to_string();
        }
        if content.contains("@main") || content.contains("@UIApplicationMain") {
            return "swift_main".to_string();
        }
        if content.contains(": View") && content.contains("var body") {
            return "swift_swiftui_view".to_string();
        }
        if content.contains(": UIViewController") {
            return "swift_view_controller".to_string();
        }
        if content.contains("protocol ") {
            return "swift_protocol".to_string();
        }
        if content.contains("class ") {
            return "swift_class".to_string();
        }
        if content.contains("struct ") {
            return "swift_struct".to_string();
        }
        if content.contains("enum ") {
            return "swift_enum".to_string();
        }
        if content.contains("extension ") {
            return "swift_extension".to_string();
        }

        "swift_file".to_string()
    }

    fn is_important_line(&self, line: &str) -> bool {
        let trimmed = line.trim();

        // Type definitions
        if trimmed.contains("class ")
            || trimmed.contains("struct ")
            || trimmed.contains("enum ")
            || trimmed.contains("protocol ")
            || trimmed.contains("extension ")
        {
            return true;
        }

        // Functions and initializers
        if trimmed.contains("func ")
            || trimmed.contains("init(")
            || trimmed.contains("init?(")
            || trimmed.contains("init!(")
        {
            return true;
        }

        // Properties with type annotation
        if (trimmed.contains("var ") || trimmed.contains("let ")) && trimmed.contains(':') {
            return true;
        }

        // Type aliases
        if trimmed.contains("typealias ") {
            return true;
        }

        // Import statements
        if trimmed.starts_with("import ") || trimmed.starts_with("@_exported import ") {
            return true;
        }

        // Attributes and property wrappers
        if trimmed.starts_with('@') {
            return true;
        }

        // Important comments
        if trimmed.contains("TODO")
            || trimmed.contains("FIXME")
            || trimmed.contains("NOTE")
            || trimmed.contains("HACK")
            || trimmed.contains("MARK:")
            || trimmed.contains("WARNING")
        {
            return true;
        }

        // Enum cases
        if trimmed.starts_with("case ") {
            return true;
        }

        false
    }

    fn language_name(&self) -> &'static str {
        "Swift"
    }

    fn extract_interfaces(&self, content: &str, _file_path: &Path) -> Vec<InterfaceInfo> {
        let mut interfaces = Vec::new();
        let lines: Vec<&str> = content.lines().collect();

        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();

            // Skip empty lines and comments
            if trimmed.is_empty() || trimmed.starts_with("//") {
                continue;
            }

            // Extract function definitions
            if self.func_regex.is_match(trimmed) {
                if let Some(name) = Self::extract_func_name(trimmed) {
                    let visibility = Self::extract_visibility(trimmed);
                    let is_async = trimmed.contains(" async ");
                    let params_str = Self::extract_params_string(trimmed);
                    let return_type = Self::extract_return_type(trimmed);

                    interfaces.push(InterfaceInfo {
                        name,
                        interface_type: if is_async {
                            "async_function"
                        } else {
                            "function"
                        }
                        .to_string(),
                        visibility,
                        parameters: Self::parse_parameters(&params_str),
                        return_type,
                        description: Self::extract_doc_comment(&lines, i),
                    });
                }
            }

            // Extract initializer definitions
            if self.init_regex.is_match(trimmed) {
                let visibility = Self::extract_visibility(trimmed);
                let _is_failable = trimmed.contains("init?") || trimmed.contains("init!");
                let params_str = Self::extract_params_string(trimmed);

                let name = if trimmed.contains("init!") {
                    "init!"
                } else if trimmed.contains("init?") {
                    "init?"
                } else {
                    "init"
                };

                interfaces.push(InterfaceInfo {
                    name: name.to_string(),
                    interface_type: "initializer".to_string(),
                    visibility,
                    parameters: Self::parse_parameters(&params_str),
                    return_type: None,
                    description: Self::extract_doc_comment(&lines, i),
                });
            }

            // Extract class definitions
            if trimmed.contains("class ") && !trimmed.contains("class func") {
                if let Some(name) = Self::extract_name_after_keyword(trimmed, "class ") {
                    let visibility = Self::extract_visibility(trimmed);
                    let is_final = trimmed.contains("final ");

                    interfaces.push(InterfaceInfo {
                        name,
                        interface_type: if is_final { "final_class" } else { "class" }.to_string(),
                        visibility,
                        parameters: Vec::new(),
                        return_type: None,
                        description: Self::extract_doc_comment(&lines, i),
                    });
                }
            }

            // Extract struct definitions
            if trimmed.contains("struct ") {
                if let Some(name) = Self::extract_name_after_keyword(trimmed, "struct ") {
                    interfaces.push(InterfaceInfo {
                        name,
                        interface_type: "struct".to_string(),
                        visibility: Self::extract_visibility(trimmed),
                        parameters: Vec::new(),
                        return_type: None,
                        description: Self::extract_doc_comment(&lines, i),
                    });
                }
            }

            // Extract protocol definitions
            if trimmed.contains("protocol ") {
                if let Some(name) = Self::extract_name_after_keyword(trimmed, "protocol ") {
                    interfaces.push(InterfaceInfo {
                        name,
                        interface_type: "protocol".to_string(),
                        visibility: Self::extract_visibility(trimmed),
                        parameters: Vec::new(),
                        return_type: None,
                        description: Self::extract_doc_comment(&lines, i),
                    });
                }
            }

            // Extract enum definitions
            if trimmed.contains("enum ") {
                if let Some(name) = Self::extract_name_after_keyword(trimmed, "enum ") {
                    let is_indirect = trimmed.contains("indirect ");
                    interfaces.push(InterfaceInfo {
                        name,
                        interface_type: if is_indirect { "indirect_enum" } else { "enum" }
                            .to_string(),
                        visibility: Self::extract_visibility(trimmed),
                        parameters: Vec::new(),
                        return_type: None,
                        description: Self::extract_doc_comment(&lines, i),
                    });
                }
            }

            // Extract extension definitions
            if trimmed.contains("extension ") {
                if let Some(name) = Self::extract_name_after_keyword(trimmed, "extension ") {
                    interfaces.push(InterfaceInfo {
                        name,
                        interface_type: "extension".to_string(),
                        visibility: Self::extract_visibility(trimmed),
                        parameters: Vec::new(),
                        return_type: None,
                        description: Self::extract_doc_comment(&lines, i),
                    });
                }
            }

            // Extract property definitions (var/let)
            if trimmed.contains("var ") || trimmed.contains("let ") {
                let is_var = trimmed.contains("var ");
                let keyword = if is_var { "var " } else { "let " };

                if let Some(name) = Self::extract_name_after_keyword(trimmed, keyword) {
                    let resembles_local = !trimmed.starts_with("let ")
                        && !trimmed.starts_with("var ")
                        && !trimmed.contains("private ")
                        && !trimmed.contains("public ")
                        && !trimmed.contains("internal ")
                        && !trimmed.contains("fileprivate ")
                        && !trimmed.contains("open ")
                        && !trimmed.contains("static ")
                        && !trimmed.contains("lazy ")
                        && !trimmed.contains("weak ")
                        && !trimmed.contains("unowned ")
                        && !trimmed.starts_with("@");

                    if resembles_local && !trimmed.contains(':') {
                        continue;
                    }

                    // Extract type - either explicit or inferred
                    let prop_type = if let Some(colon_pos) = trimmed.find(':') {
                        let after_colon = &trimmed[colon_pos + 1..];
                        let type_str: String = after_colon
                            .chars()
                            .take_while(|c| *c != '=' && *c != '{')
                            .collect();
                        let t = type_str.trim();
                        if t.is_empty() {
                            None
                        } else {
                            Some(t.to_string())
                        }
                    } else if trimmed.contains('=') {
                        // Type inference - we can try to infer basic types from the value
                        Some(Self::infer_type_from_value(trimmed))
                    } else {
                        None
                    };

                    let is_static = trimmed.contains("static ");
                    let is_lazy = trimmed.contains("lazy ");
                    let is_weak = trimmed.contains("weak ");

                    let interface_type = if is_static {
                        "static_property"
                    } else if !is_var {
                        "constant"
                    } else if is_lazy {
                        "lazy_property"
                    } else if is_weak {
                        "weak_property"
                    } else {
                        "property"
                    };

                    interfaces.push(InterfaceInfo {
                        name,
                        interface_type: interface_type.to_string(),
                        visibility: Self::extract_visibility(trimmed),
                        parameters: Vec::new(),
                        return_type: prop_type,
                        description: Self::extract_doc_comment(&lines, i),
                    });
                }
            }

            // Extract typealias definitions
            if trimmed.contains("typealias ") {
                if let Some(name) = Self::extract_name_after_keyword(trimmed, "typealias ") {
                    let aliased_type = if let Some(eq_pos) = trimmed.find('=') {
                        Some(trimmed[eq_pos + 1..].trim().to_string())
                    } else {
                        None
                    };

                    interfaces.push(InterfaceInfo {
                        name,
                        interface_type: "typealias".to_string(),
                        visibility: Self::extract_visibility(trimmed),
                        parameters: Vec::new(),
                        return_type: aliased_type,
                        description: Self::extract_doc_comment(&lines, i),
                    });
                }
            }
        }

        interfaces
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_supported_extensions() {
        let processor = SwiftProcessor::new();
        assert_eq!(processor.supported_extensions(), vec!["swift"]);
    }

    #[test]
    fn test_extract_imports() {
        let processor = SwiftProcessor::new();
        let content =
            "import Foundation\nimport UIKit\n@_exported import MyModule\nimport CustomLib";
        let deps = processor.extract_dependencies(content, &PathBuf::from("test.swift"));

        assert_eq!(deps.len(), 4);
        assert!(deps[0].is_external); // Foundation - system framework
        assert!(deps[1].is_external); // UIKit - system framework
        assert!(deps[2].is_external); // MyModule - third-party or separate target
        assert!(deps[3].is_external); // CustomLib - third-party or separate target
    }

    #[test]
    fn test_extract_function() {
        let processor = SwiftProcessor::new();
        let content =
            "/// Documentation\npublic func calculate(x: Int, y: Int) -> Int { return x + y }";
        let interfaces = processor.extract_interfaces(content, &PathBuf::from("test.swift"));

        let funcs: Vec<_> = interfaces
            .iter()
            .filter(|i| i.interface_type == "function")
            .collect();
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].name, "calculate");
        assert_eq!(funcs[0].visibility, "public");
        assert_eq!(funcs[0].parameters.len(), 2);
    }

    #[test]
    fn test_extract_async_function() {
        let processor = SwiftProcessor::new();
        let content = "func fetchData() async throws -> Data { }";
        let interfaces = processor.extract_interfaces(content, &PathBuf::from("test.swift"));

        let async_funcs: Vec<_> = interfaces
            .iter()
            .filter(|i| i.interface_type == "async_function")
            .collect();
        assert_eq!(async_funcs.len(), 1);
        assert_eq!(async_funcs[0].name, "fetchData");
    }

    #[test]
    fn test_extract_initializers() {
        let processor = SwiftProcessor::new();
        let content: &str = "init(name: String) { }\ninit?(id: Int) { }\nconvenience init() { }";
        let interfaces = processor.extract_interfaces(content, &PathBuf::from("test.swift"));

        let inits: Vec<_> = interfaces
            .iter()
            .filter(|i| i.interface_type == "initializer")
            .collect();
        assert_eq!(inits.len(), 3);
    }

    #[test]
    fn test_extract_class() {
        let processor = SwiftProcessor::new();
        let content = "public class MyClass { }\nfinal class FinalClass { }";
        let interfaces = processor.extract_interfaces(content, &PathBuf::from("test.swift"));

        assert!(
            interfaces
                .iter()
                .any(|i| i.name == "MyClass" && i.interface_type == "class")
        );
        assert!(
            interfaces
                .iter()
                .any(|i| i.name == "FinalClass" && i.interface_type == "final_class")
        );
    }

    #[test]
    fn test_extract_struct() {
        let processor = SwiftProcessor::new();
        let content = "struct User { let id: Int }";
        let interfaces = processor.extract_interfaces(content, &PathBuf::from("test.swift"));

        assert!(
            interfaces
                .iter()
                .any(|i| i.name == "User" && i.interface_type == "struct")
        );
    }

    #[test]
    fn test_extract_protocol() {
        let processor = SwiftProcessor::new();
        let content = "protocol Drawable { func draw() }";
        let interfaces = processor.extract_interfaces(content, &PathBuf::from("test.swift"));

        assert!(
            interfaces
                .iter()
                .any(|i| i.name == "Drawable" && i.interface_type == "protocol")
        );
    }

    #[test]
    fn test_extract_enum() {
        let processor = SwiftProcessor::new();
        let content = "enum Direction { case north }\nindirect enum Tree { case leaf }";
        let interfaces = processor.extract_interfaces(content, &PathBuf::from("test.swift"));

        assert!(
            interfaces
                .iter()
                .any(|i| i.name == "Direction" && i.interface_type == "enum")
        );
        assert!(
            interfaces
                .iter()
                .any(|i| i.name == "Tree" && i.interface_type == "indirect_enum")
        );
    }

    #[test]
    fn test_extract_properties() {
        let processor = SwiftProcessor::new();
        let content = "var name: String\nlet id: Int\nstatic var count: Int\nlazy var data: Data";
        let interfaces = processor.extract_interfaces(content, &PathBuf::from("test.swift"));

        assert!(
            interfaces
                .iter()
                .any(|i| i.name == "name" && i.interface_type == "property")
        );
        assert!(
            interfaces
                .iter()
                .any(|i| i.name == "id" && i.interface_type == "constant")
        );
        assert!(
            interfaces
                .iter()
                .any(|i| i.name == "count" && i.interface_type == "static_property")
        );
        assert!(
            interfaces
                .iter()
                .any(|i| i.name == "data" && i.interface_type == "lazy_property")
        );
    }

    #[test]
    fn test_extract_typealias() {
        let processor = SwiftProcessor::new();
        let content = "typealias Handler = (Int) -> Void";
        let interfaces = processor.extract_interfaces(content, &PathBuf::from("test.swift"));

        assert!(
            interfaces
                .iter()
                .any(|i| i.name == "Handler" && i.interface_type == "typealias")
        );
    }

    #[test]
    fn test_is_important_line() {
        let processor = SwiftProcessor::new();

        assert!(processor.is_important_line("func test() { }"));
        assert!(processor.is_important_line("class MyClass { }"));
        assert!(processor.is_important_line("struct MyStruct { }"));
        assert!(processor.is_important_line("// TODO: fix this"));
        assert!(processor.is_important_line("init(name: String)"));
        assert!(processor.is_important_line("var name: String"));
        assert!(processor.is_important_line("@State var count = 0"));
        assert!(!processor.is_important_line("x = 5"));
    }

    #[test]
    fn test_determine_component_type() {
        let processor = SwiftProcessor::new();

        assert_eq!(
            processor.determine_component_type(&PathBuf::from("AppDelegate.swift"), ""),
            "swift_app_delegate"
        );

        let swiftui_content = "@main struct MyApp: App { var body: some Scene { } }";
        assert_eq!(
            processor.determine_component_type(&PathBuf::from("MyApp.swift"), swiftui_content),
            "swift_swiftui_app"
        );

        let view_content = "struct ContentView: View { var body: some View { Text(\"\") } }";
        assert_eq!(
            processor.determine_component_type(&PathBuf::from("ContentView.swift"), view_content),
            "swift_swiftui_view"
        );
    }

    #[test]
    fn test_parse_parameters_with_generics() {
        let params =
            SwiftProcessor::parse_parameters("items: Array<String>, dict: Dictionary<String, Int>");
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].param_type, "Array<String>");
        assert_eq!(params[1].param_type, "Dictionary<String, Int>");
    }

    /// Test generic enum parsing (Index<T>)
    #[test]
    fn test_generic_enum_parsing() {
        let processor = SwiftProcessor::new();

        let content = r#"
public enum Index<T: Any>: Comparable {
    case array(Int)
    case dictionary(DictionaryIndex<String, T>)
    case null
}
"#;

        let interfaces = processor.extract_interfaces(content, &PathBuf::from("test.swift"));
        let enums: Vec<_> = interfaces
            .iter()
            .filter(|i| i.interface_type == "enum")
            .collect();

        assert_eq!(enums.len(), 1);
        assert_eq!(enums[0].name, "Index");
        assert_eq!(enums[0].visibility, "public");
    }

    /// Test fileprivate and internal visibility
    #[test]
    fn test_visibility_modifiers() {
        let processor = SwiftProcessor::new();

        let content = r#"
public func publicFunc() {}
private func privateFunc() {}
internal func internalFunc() {}
fileprivate func fileprivateFunc() {}
open class OpenClass {}
func defaultFunc() {}
"#;

        let interfaces = processor.extract_interfaces(content, &PathBuf::from("test.swift"));

        let public_func = interfaces.iter().find(|i| i.name == "publicFunc").unwrap();
        assert_eq!(public_func.visibility, "public");

        let private_func = interfaces.iter().find(|i| i.name == "privateFunc").unwrap();
        assert_eq!(private_func.visibility, "private");

        let internal_func = interfaces
            .iter()
            .find(|i| i.name == "internalFunc")
            .unwrap();
        assert_eq!(internal_func.visibility, "internal");

        let fileprivate_func = interfaces
            .iter()
            .find(|i| i.name == "fileprivateFunc")
            .unwrap();
        assert_eq!(fileprivate_func.visibility, "fileprivate");

        let open_class = interfaces.iter().find(|i| i.name == "OpenClass").unwrap();
        assert_eq!(open_class.visibility, "open");

        let default_func = interfaces.iter().find(|i| i.name == "defaultFunc").unwrap();
        assert_eq!(default_func.visibility, "internal");
    }

    /// Test mutating functions
    #[test]
    fn test_mutating_function() {
        let processor = SwiftProcessor::new();

        let content = r#"
public mutating func merge(with other: JSON) throws {
    try self.merge(with: other, typecheck: true)
}
"#;

        let interfaces = processor.extract_interfaces(content, &PathBuf::from("test.swift"));
        let funcs: Vec<_> = interfaces
            .iter()
            .filter(|i| i.interface_type == "function")
            .collect();

        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].name, "merge");
        assert_eq!(funcs[0].visibility, "public");
    }

    /// Test computed properties
    #[test]
    fn test_computed_properties() {
        let processor = SwiftProcessor::new();

        let content = r#"
public var errorCode: Int { return self.rawValue }

public var object: Any {
    get {
        switch type {
        case .array: return rawArray
        default: return rawNull
        }
    }
    set {
        error = nil
    }
}
"#;

        let interfaces = processor.extract_interfaces(content, &PathBuf::from("test.swift"));
        let props: Vec<_> = interfaces
            .iter()
            .filter(|i| i.interface_type == "property")
            .collect();

        assert!(props.iter().any(|p| p.name == "errorCode"));
        assert!(props.iter().any(|p| p.name == "object"));
    }

    /// Test @testable import detection
    #[test]
    fn test_testable_import() {
        let processor = SwiftProcessor::new();
        let content = r#"
@testable import RxSwift
import Foundation
@_exported import MyModule
"#;
        let deps = processor.extract_dependencies(content, &PathBuf::from("Tests.swift"));

        assert_eq!(deps.len(), 3);
        assert!(deps.iter().any(|d| d.name == "RxSwift"));
        assert!(deps.iter().any(|d| d.name == "Foundation"));
        assert!(deps.iter().any(|d| d.name == "MyModule"));
        // All imports are external
        assert!(deps.iter().all(|d| d.is_external));
    }

    /// Test @objc func detection
    #[test]
    fn test_objc_func_detection() {
        let processor = SwiftProcessor::new();
        let content = r#"
class ControlTarget: NSObject {
    @objc func eventHandler(_ sender: UIControl) {
        callback(())
    }

    @objc private func buttonTapped(_ sender: UIButton) {
        // handle tap
    }
}
"#;
        let interfaces =
            processor.extract_interfaces(content, &PathBuf::from("ControlTarget.swift"));
        let funcs: Vec<_> = interfaces
            .iter()
            .filter(|i| i.interface_type == "function")
            .collect();

        assert!(funcs.iter().any(|f| f.name == "eventHandler"));
        assert!(funcs.iter().any(|f| f.name == "buttonTapped"));
    }

    /// Test @available decorated functions
    #[test]
    fn test_available_decorated_func() {
        let processor = SwiftProcessor::new();
        let content = r#"
@available(iOS 13.0, macOS 10.15, *)
public func asyncMethod() async throws -> String {
    return "result"
}

@available(*, deprecated, message: "Use newMethod instead")
func oldMethod() -> Int {
    return 0
}

@available(iOS 14.0, *) @MainActor public func mainActorMethod() {
    // runs on main actor
}
"#;
        let interfaces = processor.extract_interfaces(content, &PathBuf::from("API.swift"));
        let funcs: Vec<_> = interfaces
            .iter()
            .filter(|i| i.interface_type == "function" || i.interface_type == "async_function")
            .collect();

        assert!(funcs.iter().any(|f| f.name == "asyncMethod"));
        assert!(funcs.iter().any(|f| f.name == "oldMethod"));
        assert!(funcs.iter().any(|f| f.name == "mainActorMethod"));
    }

    /// Test generic initializer detection
    #[test]
    fn test_generic_initializer_detection() {
        let processor = SwiftProcessor::new();
        let content = r#"
public struct Binder<Value>: ObserverType {
    public init<Target: AnyObject>(_ target: Target, scheduler: ImmediateSchedulerType = MainScheduler(), binding: @escaping (Target, Value) -> Void) {
        // implementation
    }

    public init<Target: AnyObject>(target: Target, binding: @escaping (Target, Value) -> Void) where Target: Sendable {
        // implementation
    }
}
"#;
        let interfaces = processor.extract_interfaces(content, &PathBuf::from("Binder.swift"));
        let inits: Vec<_> = interfaces
            .iter()
            .filter(|i| i.interface_type == "initializer")
            .collect();

        // Now generic initializers should be detected
        assert!(
            inits.len() >= 2,
            "Expected at least 2 initializers, found {}",
            inits.len()
        );
    }

    /// Test final func detection
    #[test]
    fn test_final_func_detection() {
        let processor = SwiftProcessor::new();
        let content = r#"
public final class PublishSubject {
    final func synchronized_dispose() {
        self.disposed = true
    }

    public final func finalPublicMethod() {
        // implementation
    }
}
"#;
        let interfaces =
            processor.extract_interfaces(content, &PathBuf::from("PublishSubject.swift"));
        let funcs: Vec<_> = interfaces
            .iter()
            .filter(|i| i.interface_type == "function")
            .collect();

        assert!(funcs.iter().any(|f| f.name == "synchronized_dispose"));
        assert!(funcs.iter().any(|f| f.name == "finalPublicMethod"));
    }

    /// Test property type inference
    #[test]
    fn test_property_type_inference() {
        let processor = SwiftProcessor::new();
        let content = r#"
class MyClass {
    private let lock = RecursiveLock()
    private var disposed = false
    private var observers = Observers()
    private var stopped = false
    fileprivate var rawArray: [Any] = []
    let stringValue = "hello"
    var intValue = 42
    var doubleValue = 3.14
    static let shared = MyClass()
}
"#;
        let interfaces = processor.extract_interfaces(content, &PathBuf::from("MyClass.swift"));
        let props: Vec<_> = interfaces
            .iter()
            .filter(|i| {
                i.interface_type == "property"
                    || i.interface_type == "constant"
                    || i.interface_type == "static_property"
            })
            .collect();

        // Check that properties with type inference are detected
        assert!(props.iter().any(|p| p.name == "lock"));
        assert!(props.iter().any(|p| p.name == "disposed"));
        assert!(props.iter().any(|p| p.name == "observers"));
        assert!(props.iter().any(|p| p.name == "stopped"));
        assert!(props.iter().any(|p| p.name == "rawArray"));
        assert!(props.iter().any(|p| p.name == "stringValue"));
        assert!(props.iter().any(|p| p.name == "intValue"));
        assert!(props.iter().any(|p| p.name == "doubleValue"));
        assert!(props.iter().any(|p| p.name == "shared"));

        // Check inferred types
        let lock_prop = props.iter().find(|p| p.name == "lock").unwrap();
        assert_eq!(lock_prop.return_type.as_ref().unwrap(), "RecursiveLock");

        let disposed_prop = props.iter().find(|p| p.name == "disposed").unwrap();
        assert_eq!(disposed_prop.return_type.as_ref().unwrap(), "Bool");

        let string_prop = props.iter().find(|p| p.name == "stringValue").unwrap();
        assert_eq!(string_prop.return_type.as_ref().unwrap(), "String");

        let int_prop = props.iter().find(|p| p.name == "intValue").unwrap();
        assert_eq!(int_prop.return_type.as_ref().unwrap(), "Int");

        let double_prop = props.iter().find(|p| p.name == "doubleValue").unwrap();
        assert_eq!(double_prop.return_type.as_ref().unwrap(), "Double");
    }
}
