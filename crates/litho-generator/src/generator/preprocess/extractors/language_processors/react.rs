use super::{Dependency, LanguageProcessor};
use crate::types::code::InterfaceInfo;
use regex::Regex;
use std::path::Path;

#[derive(Debug)]
pub struct ReactProcessor {
    import_regex: Regex,
    hook_regex: Regex,
}

impl ReactProcessor {
    pub fn new() -> Self {
        Self {
            import_regex: Regex::new(r#"^\s*import\s+(?:.*\s+from\s+)?['"]([^'"]+)['"]"#).unwrap(),
            hook_regex: Regex::new(r"use[A-Z][a-zA-Z]*\s*\(").unwrap(),
        }
    }
}

impl LanguageProcessor for ReactProcessor {
    fn supported_extensions(&self) -> Vec<&'static str> {
        vec!["jsx", "tsx"]
    }

    fn extract_dependencies(&self, content: &str, file_path: &Path) -> Vec<Dependency> {
        let mut dependencies = Vec::new();
        let source_file = file_path.to_string_lossy().to_string();

        for (line_num, line) in content.lines().enumerate() {
            // Extract import statements
            if let Some(captures) = self.import_regex.captures(line) {
                if let Some(import_path) = captures.get(1) {
                    let path_str = import_path.as_str();
                    let is_external = !path_str.starts_with('.')
                        && !path_str.starts_with('/')
                        && !path_str.starts_with("@/");

                    let dependency_type = if path_str == "react" || path_str.starts_with("react/") {
                        "react_import"
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
        if file_name == "App.jsx" || file_name == "App.tsx" {
            return "react_app".to_string();
        }

        if file_name == "index.jsx" || file_name == "index.tsx" {
            return "react_entry".to_string();
        }

        if file_name.to_lowercase().contains("page")
            || file_path.to_string_lossy().contains("/pages/")
        {
            return "react_page".to_string();
        }

        if file_name.to_lowercase().contains("hook") || file_name.starts_with("use") {
            return "react_hook".to_string();
        }

        // Check content patterns
        if content.contains("export default")
            && (content.contains("return (") || content.contains("return <"))
        {
            "react_component".to_string()
        } else if self.hook_regex.is_match(content) {
            "react_hook".to_string()
        } else if content.contains("createContext") || content.contains("useContext") {
            "react_context".to_string()
        } else if content.contains("reducer") || content.contains("useReducer") {
            "react_reducer".to_string()
        } else {
            "react_utility".to_string()
        }
    }

    fn is_important_line(&self, line: &str) -> bool {
        let trimmed = line.trim();

        // React component definitions
        if trimmed.starts_with("function ")
            && (trimmed.contains("()") || trimmed.contains("(props"))
            || trimmed.starts_with("const ") && trimmed.contains("= (") && trimmed.contains("=>")
        {
            return true;
        }

        // React Hooks
        if trimmed.contains("useState")
            || trimmed.contains("useEffect")
            || trimmed.contains("useContext")
            || trimmed.contains("useReducer")
            || trimmed.contains("useMemo")
            || trimmed.contains("useCallback")
            || self.hook_regex.is_match(trimmed)
        {
            return true;
        }

        // JSX return statements
        if trimmed.starts_with("return (") || trimmed.starts_with("return <") {
            return true;
        }

        // Import/export statements
        if trimmed.starts_with("import ") || trimmed.starts_with("export ") {
            return true;
        }

        // React-specific patterns
        if trimmed.contains("createContext")
            || trimmed.contains("forwardRef")
            || trimmed.contains("memo(")
            || trimmed.contains("lazy(")
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
        "React"
    }

    fn extract_interfaces(&self, content: &str, _file_path: &Path) -> Vec<InterfaceInfo> {
        let mut interfaces = Vec::new();
        let lines: Vec<&str> = content.lines().collect();

        // React component interface analysis focuses on component definitions and Hook usage
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();

            // Extract function component definitions
            if let Some(component_name) = self.extract_function_component(trimmed) {
                interfaces.push(InterfaceInfo {
                    name: component_name,
                    interface_type: "react_component".to_string(),
                    visibility: "public".to_string(),
                    parameters: Vec::new(),
                    return_type: Some("JSX.Element".to_string()),
                    description: self.extract_component_comment(&lines, i),
                });
            }

            // Extract class component definitions
            if let Some(component_name) = self.extract_class_component(trimmed) {
                interfaces.push(InterfaceInfo {
                    name: component_name,
                    interface_type: "react_class_component".to_string(),
                    visibility: "public".to_string(),
                    parameters: Vec::new(),
                    return_type: Some("JSX.Element".to_string()),
                    description: self.extract_component_comment(&lines, i),
                });
            }

            // Extract custom Hook definitions
            if let Some(hook_name) = self.extract_custom_hook(trimmed) {
                interfaces.push(InterfaceInfo {
                    name: hook_name,
                    interface_type: "react_hook".to_string(),
                    visibility: "public".to_string(),
                    parameters: Vec::new(),
                    return_type: None,
                    description: self.extract_component_comment(&lines, i),
                });
            }
        }

        interfaces
    }
}

impl ReactProcessor {
    /// Extract function component name
    fn extract_function_component(&self, line: &str) -> Option<String> {
        // Match: function ComponentName, const ComponentName = (), export function ComponentName
        if line.contains("function") && (line.contains("return") || line.contains("=>")) {
            if let Some(start) = line.find("function") {
                let after_function = &line[start + 8..].trim();
                if let Some(space_pos) = after_function.find(' ') {
                    let name = after_function[..space_pos].trim();
                    if name.chars().next().map_or(false, |c| c.is_uppercase()) {
                        return Some(name.to_string());
                    }
                }
            }
        }

        // Match: const ComponentName = () => or const ComponentName: React.FC
        if line.starts_with("const") || line.starts_with("export const") {
            if let Some(eq_pos) = line.find('=') {
                let before_eq = &line[..eq_pos];
                if let Some(name_start) = before_eq.rfind(' ') {
                    let name = before_eq[name_start + 1..].trim().trim_end_matches(':');
                    if name.chars().next().map_or(false, |c| c.is_uppercase()) {
                        return Some(name.to_string());
                    }
                }
            }
        }

        None
    }

    /// Extract class component name
    fn extract_class_component(&self, line: &str) -> Option<String> {
        if line.contains("class")
            && (line.contains("extends React.Component") || line.contains("extends Component"))
        {
            if let Some(class_pos) = line.find("class") {
                let after_class = &line[class_pos + 5..].trim();
                if let Some(space_pos) = after_class.find(' ') {
                    let name = after_class[..space_pos].trim();
                    if name.chars().next().map_or(false, |c| c.is_uppercase()) {
                        return Some(name.to_string());
                    }
                }
            }
        }
        None
    }

    /// Extract custom Hook name
    fn extract_custom_hook(&self, line: &str) -> Option<String> {
        // Match: function useCustomHook, const useCustomHook =
        if line.contains("function use") || (line.contains("const use") && line.contains('=')) {
            if line.contains("function") {
                if let Some(start) = line.find("function") {
                    let after_function = &line[start + 8..].trim();
                    if let Some(space_pos) = after_function.find(' ') {
                        let name = after_function[..space_pos].trim();
                        if name.starts_with("use") && name.len() > 3 {
                            return Some(name.to_string());
                        }
                    }
                }
            } else if line.contains("const") {
                if let Some(eq_pos) = line.find('=') {
                    let before_eq = &line[..eq_pos];
                    if let Some(name_start) = before_eq.rfind(' ') {
                        let name = before_eq[name_start + 1..].trim();
                        if name.starts_with("use") && name.len() > 3 {
                            return Some(name.to_string());
                        }
                    }
                }
            }
        }
        None
    }

    /// Extract component comment
    fn extract_component_comment(&self, lines: &[&str], current_line: usize) -> Option<String> {
        let mut doc_lines = Vec::new();

        // Search upward for comments
        for i in (0..current_line).rev() {
            let line = lines[i].trim();

            if line.starts_with("//") {
                doc_lines.insert(0, line.trim_start_matches("//").trim().to_string());
            } else if line.starts_with("/*") && line.ends_with("*/") {
                let content = line.trim_start_matches("/*").trim_end_matches("*/").trim();
                doc_lines.insert(0, content.to_string());
                break;
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
