use super::{Dependency, LanguageProcessor};
use crate::types::code::{InterfaceInfo, ParameterInfo};
use regex::Regex;
use serde_json::Value;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::Path;

#[derive(Debug)]
pub struct PhpProcessor {
    /// Regex to capture PHP namespace declarations.
    namespace_regex: Regex,
    /// Regex to capture PHP use statements.
    use_regex: Regex,
    /// Regex to capture PHP require/include statements.
    dependency_keyword_regex: Regex,
    /// Regex to capture Composer-style comments (e.g., `// composer: package/name`).
    composer_regex: Regex,
    /// Regex to capture PHP class declarations (including abstract/final).
    class_regex: Regex,
    /// Regex to capture PHP trait declarations.
    trait_regex: Regex,
    /// Regex to capture PHP interface declarations.
    interface_regex: Regex,
    /// Regex to capture global PHP function declarations.
    function_regex: Regex,
    /// Regex to capture PHP method declarations (including visibility, static, abstract/final, and reference).
    method_regex: Regex,
    /// Regex to capture PHP enum declarations (PHP 8.1+).
    enum_regex: Regex,
    /// Set of normalized namespace prefixes treated as internal
    internal_namespaces: HashSet<String>,
}

impl PhpProcessor {
    pub fn new() -> Self {
        Self {
            namespace_regex: Regex::new(r"^\s*namespace\s+([^;]+);").unwrap(),
            use_regex: Regex::new(r"^\s*use\s+(?:function\s+|const\s+)?([^;]+);").unwrap(),
            dependency_keyword_regex: Regex::new(r#"^\s*(require_once|require|include_once|include)\b"#).unwrap(),
            composer_regex: Regex::new(r#"(?i)(?://|#)\s*composer:\s*(.*)"#).unwrap(),
            class_regex: Regex::new(r"^\s*((?:abstract\s+|final\s+|readonly\s+)?)(class)\s+(\w+)").unwrap(),
            trait_regex: Regex::new(r"^\s*trait\s+(\w+)").unwrap(),
            interface_regex: Regex::new(r"^\s*interface\s+(\w+)").unwrap(),
            function_regex: Regex::new(r"^\s*function\s+(\w+)\s*\(([^)]*)\)\s*(?::\s*([^{;]+))?").unwrap(),
            method_regex: Regex::new(r"^\s*(?:(public|protected|private)\s+)?(?:(static)\s+)?(?:(abstract|final)\s+)?function\s+(&)?\s*(\w+)\s*\(([^)]*)\)\s*(?::\s*([^{;]+))?").unwrap(),
            enum_regex: Regex::new(r"^\s*enum\s+(\w+)(?:\s*:\s*\w+)?\s*\{?").unwrap(),
            internal_namespaces: Self::detect_internal_namespaces(),
        }
    }
}

impl LanguageProcessor for PhpProcessor {
    fn supported_extensions(&self) -> Vec<&'static str> {
        vec!["php"]
    }

    /// Extracts dependencies from the PHP content by parsing namespace, use, require, include,
    /// and composer-style comments.
    fn extract_dependencies(&self, content: &str, file_path: &Path) -> Vec<Dependency> {
        let mut dependencies = Vec::new();
        let source_file = file_path.to_string_lossy().to_string();

        let mut line_iter = content.lines().enumerate();
        while let Some((line_num, line)) = line_iter.next() {
            let trimmed = line.trim();
            if trimmed.starts_with("use ") {
                let mut statement = trimmed.to_string();
                while !statement.trim_end().ends_with(';') {
                    if let Some((_, next_line)) = line_iter.next() {
                        statement.push(' ');
                        statement.push_str(next_line.trim());
                    } else {
                        break;
                    }
                }
                self.extract_use_dependency(&statement, line_num, &source_file, &mut dependencies);
                continue;
            }

            self.extract_namespace_dependency(line, line_num, &source_file, &mut dependencies);
            self.extract_keyword_dependency(line, line_num, &source_file, &mut dependencies);
            self.extract_composer_dependency(line, line_num, &source_file, &mut dependencies);
        }

        dependencies
    }

    /// Determines the type of PHP component based on file content.
    /// This is a simplified heuristic and might not cover all edge cases.
    fn determine_component_type(&self, _file_path: &Path, content: &str) -> String {
        if self.interface_regex.is_match(content) {
            "php_interface".to_string()
        } else if self.trait_regex.is_match(content) {
            "php_trait".to_string()
        } else if self.enum_regex.is_match(content) {
            "php_enum".to_string()
        } else if self.class_regex.is_match(content) {
            "php_class".to_string()
        } else {
            "php_file".to_string()
        }
    }

    /// Identifies if a given line of PHP code is considered "important".
    /// Important lines typically define structure, control flow, or contain significant metadata.
    fn is_important_line(&self, line: &str) -> bool {
        let trimmed = line.trim();

        // Strip PHP open tag variations for consistent matching.
        let processed_line = trimmed
            .strip_prefix("<?php")
            .or_else(|| trimmed.strip_prefix("<?"))
            .map(|s| s.trim_start())
            .unwrap_or(trimmed);

        if self.class_regex.is_match(processed_line)
            || self.trait_regex.is_match(processed_line)
            || self.interface_regex.is_match(processed_line)
            || self.enum_regex.is_match(processed_line)
            || self.function_regex.is_match(processed_line)
            || self.method_regex.is_match(processed_line)
            || self.namespace_regex.is_match(processed_line)
            || self.use_regex.is_match(processed_line)
            || self.dependency_keyword_regex.is_match(processed_line)
            || processed_line.starts_with("/**")
            || processed_line.starts_with('*')
            || processed_line.starts_with("#[")
        {
            return true;
        }

        // Important comments (TODO, FIXME, etc.)
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
        "PHP"
    }

    /// Extracts interface information (classes, traits, interfaces, functions, methods, enums)
    /// from the PHP content.
    fn extract_interfaces(&self, content: &str, _file_path: &Path) -> Vec<InterfaceInfo> {
        let mut interfaces = Vec::new();
        let lines: Vec<&str> = content.lines().collect();
        let mut in_class_body = false;
        let mut brace_level = 0;

        for (i, line) in lines.iter().enumerate() {
            if self.class_regex.is_match(line)
                || self.trait_regex.is_match(line)
                || self.interface_regex.is_match(line)
            {
                in_class_body = true;
            }

            if in_class_body {
                brace_level += line.chars().filter(|&c| c == '{').count();
                brace_level -= line.chars().filter(|&c| c == '}').count();
                if brace_level == 0 {
                    in_class_body = false;
                }
            }

            self.extract_class_interface(line, &lines, i, &mut interfaces);
            self.extract_trait_interface(line, &lines, i, &mut interfaces);
            self.extract_interface_interface(line, &lines, i, &mut interfaces);
            self.extract_enum_interface(line, &lines, i, &mut interfaces);
            if in_class_body {
                self.extract_method_interface(line, &lines, i, &mut interfaces);
            } else {
                self.extract_function_interface(line, &lines, i, &mut interfaces);
            }
        }
        interfaces
    }
}

impl PhpProcessor {
    fn extract_class_interface<'a>(
        &self,
        line: &str,
        lines: &[&str],
        i: usize,
        interfaces: &mut Vec<InterfaceInfo>,
    ) {
        if let Some(captures) = self.class_regex.captures(line) {
            let name = captures
                .get(3)
                .map(|m| m.as_str())
                .unwrap_or("")
                .to_string();
            let prefix = captures.get(1).map(|m| m.as_str().trim()).unwrap_or("");
            let interface_type = if prefix.is_empty() {
                "class".to_string()
            } else {
                format!("{} class", prefix)
            };

            interfaces.push(InterfaceInfo {
                name,
                interface_type,
                visibility: "public".to_string(),
                parameters: Vec::new(),
                return_type: None,
                description: self.describe_element(lines, i),
            });
        }
    }

    fn extract_trait_interface(
        &self,
        line: &str,
        lines: &[&str],
        i: usize,
        interfaces: &mut Vec<InterfaceInfo>,
    ) {
        if let Some(captures) = self.trait_regex.captures(line) {
            let name = captures
                .get(1)
                .map(|m| m.as_str())
                .unwrap_or("")
                .to_string();

            interfaces.push(InterfaceInfo {
                name,
                interface_type: "trait".to_string(),
                visibility: "public".to_string(),
                parameters: Vec::new(),
                return_type: None,
                description: self.describe_element(lines, i),
            });
        }
    }

    fn extract_interface_interface(
        &self,
        line: &str,
        lines: &[&str],
        i: usize,
        interfaces: &mut Vec<InterfaceInfo>,
    ) {
        if let Some(captures) = self.interface_regex.captures(line) {
            let name = captures
                .get(1)
                .map(|m| m.as_str())
                .unwrap_or("")
                .to_string();

            interfaces.push(InterfaceInfo {
                name,
                interface_type: "interface".to_string(),
                visibility: "public".to_string(),
                parameters: Vec::new(),
                return_type: None,
                description: self.describe_element(lines, i),
            });
        }
    }

    fn extract_enum_interface(
        &self,
        line: &str,
        lines: &[&str],
        i: usize,
        interfaces: &mut Vec<InterfaceInfo>,
    ) {
        if line.contains("case ") {
            return;
        }
        if let Some(captures) = self.enum_regex.captures(line) {
            let name = captures
                .get(1)
                .map(|m| m.as_str())
                .unwrap_or("")
                .to_string();

            interfaces.push(InterfaceInfo {
                name,
                interface_type: "enum".to_string(),
                visibility: "public".to_string(),
                parameters: Vec::new(),
                return_type: None,
                description: self.describe_element(lines, i),
            });
        }
    }

    fn extract_function_interface(
        &self,
        line: &str,
        lines: &[&str],
        i: usize,
        interfaces: &mut Vec<InterfaceInfo>,
    ) {
        if let Some(captures) = self.function_regex.captures(line) {
            let name = captures
                .get(1)
                .map(|m| m.as_str())
                .unwrap_or("")
                .to_string();
            let params_str = captures.get(2).map(|m| m.as_str()).unwrap_or("");
            let return_type = captures.get(3).map(|m| m.as_str().trim().to_string());

            let parameters = self.parse_php_parameters(params_str);

            interfaces.push(InterfaceInfo {
                name,
                interface_type: "function".to_string(),
                visibility: "public".to_string(),
                parameters,
                return_type,
                description: self.describe_element(lines, i),
            });
        }
    }

    fn extract_method_interface(
        &self,
        line: &str,
        lines: &[&str],
        i: usize,
        interfaces: &mut Vec<InterfaceInfo>,
    ) {
        if let Some(captures) = self.method_regex.captures(line) {
            let visibility = captures.get(1).map_or("public", |m| m.as_str()).to_string();
            let name = captures
                .get(5)
                .map(|m| m.as_str())
                .unwrap_or("")
                .to_string();
            let params_str = captures.get(6).map(|m| m.as_str()).unwrap_or("");
            let return_type = captures.get(7).map(|m| m.as_str().trim().to_string());

            let parameters = self.parse_php_parameters(params_str);

            interfaces.push(InterfaceInfo {
                name,
                interface_type: "method".to_string(),
                visibility,
                parameters,
                return_type,
                description: self.describe_element(lines, i),
            });
        }
    }

    fn extract_namespace_dependency(
        &self,
        line: &str,
        line_num: usize,
        source_file: &str,
        dependencies: &mut Vec<Dependency>,
    ) {
        if let Some(captures) = self.namespace_regex.captures(line) {
            if let Some(namespace) = captures.get(1) {
                dependencies.push(Dependency {
                    name: namespace.as_str().trim().to_string(),
                    path: Some(source_file.to_string()),
                    is_external: false,
                    line_number: Some(line_num + 1),
                    dependency_type: "namespace".to_string(),
                    version: None,
                });
            }
        }
    }

    fn extract_use_dependency(
        &self,
        line: &str,
        line_num: usize,
        source_file: &str,
        dependencies: &mut Vec<Dependency>,
    ) {
        if let Some(captures) = self.use_regex.captures(line) {
            if let Some(use_segment) = captures.get(1) {
                for import_path in self.iter_use_entries(use_segment.as_str()) {
                    dependencies.push(Dependency {
                        name: import_path.clone(),
                        path: Some(source_file.to_string()),
                        is_external: !self.is_internal_namespace(&import_path),
                        line_number: Some(line_num + 1),
                        dependency_type: "use".to_string(),
                        version: None,
                    });
                }
            }
        }
    }

    fn extract_keyword_dependency(
        &self,
        line: &str,
        line_num: usize,
        source_file: &str,
        dependencies: &mut Vec<Dependency>,
    ) {
        if let Some(captures) = self.dependency_keyword_regex.captures(line) {
            let dependency_type = captures.get(1).unwrap().as_str().to_string();
            let mut path_expr = line[captures.get(0).unwrap().end()..].trim();

            if path_expr.starts_with('(') {
                path_expr = path_expr[1..].trim_start();
            }

            path_expr = path_expr.trim_end_matches(';').trim_end();

            if path_expr.ends_with(')') {
                path_expr = path_expr[..path_expr.len() - 1].trim_end();
            }

            let dependency_name = Self::strip_quotes(path_expr);
            if !dependency_name.is_empty() {
                dependencies.push(Dependency {
                    name: dependency_name,
                    path: Some(source_file.to_string()),
                    is_external: false,
                    line_number: Some(line_num + 1),
                    dependency_type,
                    version: None,
                });
            }
        }
    }

    fn extract_composer_dependency(
        &self,
        line: &str,
        line_num: usize,
        source_file: &str,
        dependencies: &mut Vec<Dependency>,
    ) {
        if let Some(captures) = self.composer_regex.captures(line) {
            if let Some(composer_info) = captures.get(1) {
                let composer_info = composer_info.as_str().trim();
                if !composer_info.is_empty() {
                    dependencies.push(Dependency {
                        name: composer_info.to_string(),
                        path: Some(source_file.to_string()),
                        is_external: true,
                        line_number: Some(line_num + 1),
                        dependency_type: "composer".to_string(),
                        version: None,
                    });
                }
            }
        }
    }

    /// Parses PHP function parameters.
    fn parse_php_parameters(&self, params_str: &str) -> Vec<ParameterInfo> {
        let mut parameters = Vec::new();
        if params_str.trim().is_empty() {
            return parameters;
        }

        let mut current_param = String::new();
        let mut paren_depth = 0;
        let mut bracket_depth = 0;

        for char in params_str.chars() {
            match char {
                '(' => paren_depth += 1,
                ')' => paren_depth -= 1,
                '[' => bracket_depth += 1,
                ']' => bracket_depth -= 1,
                ',' if paren_depth == 0 && bracket_depth == 0 => {
                    self.parse_single_php_parameter(&current_param, &mut parameters);
                    current_param.clear();
                    continue;
                }
                _ => {}
            }
            current_param.push(char);
        }

        if !current_param.is_empty() {
            self.parse_single_php_parameter(&current_param, &mut parameters);
        }

        parameters
    }

    fn parse_single_php_parameter(&self, param: &str, parameters: &mut Vec<ParameterInfo>) {
        let param = param.trim();
        if param.is_empty() {
            return;
        }

        let cleaned_param = Self::strip_attributes_prefix(param);
        if cleaned_param.is_empty() {
            return;
        }

        let is_optional = cleaned_param.contains('=');
        let name_start = cleaned_param.find('$');

        let (param_type, name) = if let Some(idx) = name_start {
            let type_segment = cleaned_param[..idx].trim().trim_end_matches('&').trim();
            let mut param_type = "mixed".to_string();
            if !type_segment.is_empty() {
                let mut tokens: Vec<_> = type_segment.split_whitespace().collect();
                tokens.retain(|token| {
                    !matches!(
                        token.to_ascii_lowercase().as_str(),
                        "public" | "protected" | "private" | "readonly"
                    )
                });
                let cleaned_type = tokens.join(" ");
                if !cleaned_type.trim().is_empty() {
                    param_type = cleaned_type;
                }
            }

            let remainder = &cleaned_param[idx..];
            let name = remainder
                .split('=')
                .next()
                .unwrap_or("")
                .trim_start_matches('$')
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_string();

            (param_type, name)
        } else {
            let name = cleaned_param
                .split('=')
                .next()
                .unwrap_or("")
                .trim_start_matches('$')
                .to_string();
            ("mixed".to_string(), name)
        };

        if name.is_empty() {
            return;
        }

        parameters.push(ParameterInfo {
            name,
            param_type,
            is_optional,
            description: None,
        });
    }

    /// Iterates over each resolved import path, expanding grouped `use` syntax like `Foo\{Bar, Baz}`.
    fn iter_use_entries(&self, segment: &str) -> Vec<String> {
        let mut parts = Vec::new();
        let mut buffer = String::new();
        let mut depth: usize = 0;

        for ch in segment.chars() {
            match ch {
                '{' => {
                    depth += 1;
                    buffer.push(ch);
                }
                '}' => {
                    depth = depth.saturating_sub(1);
                    buffer.push(ch);
                }
                ',' if depth == 0 => {
                    parts.push(buffer.trim().to_string());
                    buffer.clear();
                }
                _ => buffer.push(ch),
            }
        }

        if !buffer.trim().is_empty() {
            parts.push(buffer.trim().to_string());
        }

        let mut results = Vec::new();
        let mut seen = HashSet::new();

        for part in parts {
            for entry in self.expand_use_part(&part) {
                if seen.insert(entry.clone()) {
                    results.push(entry);
                }
            }
        }

        results
    }

    fn expand_use_part(&self, part: &str) -> Vec<String> {
        let part = part.trim();
        if part.is_empty() {
            return Vec::new();
        }

        let mut entries = Vec::new();
        if let Some(open_brace) = part.find('{') {
            if let Some(close_brace) = part.rfind('}') {
                let prefix = part[..open_brace].trim().trim_end_matches('\\').trim();
                let inner = &part[open_brace + 1..close_brace];

                for inner_entry in inner.split(',') {
                    let entry = inner_entry.trim();
                    if entry.is_empty() {
                        continue;
                    }
                    let combined = if prefix.is_empty() {
                        Self::strip_use_alias(entry)
                    } else {
                        format!(
                            "{}\\{}",
                            prefix.trim_end_matches('\\'),
                            Self::strip_use_alias(entry)
                        )
                    };
                    let cleaned = Self::strip_use_alias(&combined);
                    if !cleaned.is_empty() {
                        entries.push(cleaned);
                    }
                }
                return entries;
            }
        }

        let cleaned = Self::strip_use_alias(part);
        if !cleaned.is_empty() {
            entries.push(cleaned);
        }

        entries
    }

    /// Removes any `as alias` suffix from a use entry.
    fn strip_use_alias(entry: &str) -> String {
        entry
            .split(" as ")
            .next()
            .unwrap_or(entry)
            .trim()
            .trim_start_matches('\\')
            .to_string()
    }

    /// Removes leading attribute blocks (`#[...]`) from a parameter definition.
    fn strip_attributes_prefix(param: &str) -> &str {
        let mut trimmed = param.trim();

        while trimmed.starts_with("#[") {
            if let Some(end) = trimmed.find(']') {
                trimmed = trimmed[end + 1..].trim();
            } else {
                break;
            }
        }

        trimmed
    }

    /// Gathers contiguous attribute lines preceding a declaration.
    fn collect_attributes(&self, lines: &[&str], current_line: usize) -> Vec<String> {
        let mut attributes = Vec::new();

        for i in (0..current_line).rev() {
            let trimmed = lines[i].trim();
            if trimmed.starts_with("#[") {
                attributes.insert(0, trimmed.to_string());
                continue;
            }

            if trimmed.is_empty() || trimmed.starts_with("/**") || trimmed.starts_with('*') {
                continue;
            }

            break;
        }

        attributes
    }

    /// Builds the description by combining docblocks and attributes.
    fn describe_element(&self, lines: &[&str], current_line: usize) -> Option<String> {
        let mut parts = Vec::new();

        if let Some(doc) = self.extract_docblock(lines, current_line) {
            parts.push(doc);
        }

        let attribute_lines = self.collect_attributes(lines, current_line);
        if !attribute_lines.is_empty() {
            parts.push(format!("Attributes: {}", attribute_lines.join(" ")));
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join(" | "))
        }
    }

    fn strip_quotes(entry: &str) -> String {
        let trimmed = entry.trim();
        if trimmed.len() >= 2 {
            let bytes = trimmed.as_bytes();
            if (bytes[0] == b'\'' && bytes[trimmed.len() - 1] == b'\'')
                || (bytes[0] == b'"' && bytes[trimmed.len() - 1] == b'"')
            {
                return trimmed[1..trimmed.len() - 1].to_string();
            }
        }
        trimmed.to_string()
    }

    fn detect_internal_namespaces() -> HashSet<String> {
        let mut namespaces: HashSet<String> = ["app", "src", "lib", "core", "domain", "infra"]
            .into_iter()
            .map(|segment| segment.to_string())
            .collect();

        if let Ok(project_root) = env::current_dir() {
            for namespace in Self::load_namespaces_from_composer(&project_root) {
                if !namespace.is_empty() {
                    namespaces.insert(namespace);
                }
            }
        }

        namespaces
    }

    fn load_namespaces_from_composer(project_root: &Path) -> HashSet<String> {
        let mut namespaces = HashSet::new();
        let composer_path = project_root.join("composer.json");

        if let Ok(contents) = fs::read_to_string(composer_path) {
            if let Ok(value) = serde_json::from_str::<Value>(&contents) {
                Self::collect_psr_namespaces(&value, "autoload", &mut namespaces);
                Self::collect_psr_namespaces(&value, "autoload-dev", &mut namespaces);
            }
        }

        namespaces
    }

    fn collect_psr_namespaces(value: &Value, section: &str, collected: &mut HashSet<String>) {
        if let Some(loader_section) = value.get(section) {
            for psr in ["psr-4", "psr-0"] {
                if let Some(map) = loader_section.get(psr).and_then(|v| v.as_object()) {
                    for namespace in map.keys() {
                        let normalized = Self::normalize_namespace(namespace);
                        if !normalized.is_empty() {
                            collected.insert(normalized);
                        }
                    }
                }
            }
        }
    }

    /// Heuristically determines whether a namespace is internal (app-owned) or third-party.
    fn is_internal_namespace(&self, namespace: &str) -> bool {
        let first_segment = Self::normalize_namespace(namespace);
        !first_segment.is_empty() && self.internal_namespaces.contains(&first_segment)
    }

    /// Normalize namespace strings so we can compare prefixes consistently.
    fn normalize_namespace(namespace: &str) -> String {
        namespace
            .trim_end_matches('\\')
            .split('\\')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase()
    }

    /// Extracts PHP DocBlock comments preceding a code element.
    /// Filters out @param, @return, and other tags, retaining only the descriptive text.
    fn extract_docblock(&self, lines: &[&str], current_line: usize) -> Option<String> {
        let mut doc_lines = Vec::new();
        let mut in_docblock = false;

        for i in (0..current_line).rev() {
            let line = lines[i].trim();

            if line.ends_with("*/") {
                in_docblock = true;
                if line.starts_with("/**") {
                    let content = line.trim_start_matches("/**").trim_end_matches("*/").trim();
                    if !content.is_empty() && !content.starts_with('@') {
                        doc_lines.insert(0, content.to_string());
                    }
                    break;
                } else {
                    let content = line.trim_end_matches("*/").trim();
                    if !content.is_empty() && content != "*" && !content.starts_with('@') {
                        doc_lines.insert(0, content.trim_start_matches('*').trim().to_string());
                    }
                }
            } else if in_docblock {
                if line.starts_with("/**") {
                    let content = line.trim_start_matches("/**").trim();
                    if !content.is_empty() && !content.starts_with('@') {
                        doc_lines.insert(0, content.to_string());
                    }
                    break;
                } else if line.starts_with('*') {
                    let content = line.trim_start_matches('*').trim();
                    // Filter out DocBlock tags like @param, @return
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::preprocess::extractors::language_processors::LanguageProcessorManager;

    #[test]
    fn test_supported_extensions() {
        let processor = PhpProcessor::new();
        assert_eq!(processor.supported_extensions(), vec!["php"]);
    }

    #[test]
    fn test_language_name() {
        let processor = PhpProcessor::new();
        assert_eq!(processor.language_name(), "PHP");
    }

    #[test]
    fn test_extract_dependencies() {
        let processor = PhpProcessor::new();
        let content = r#"<?php
namespace App\Http\Controllers;

use App\Models\User;
use Illuminate\Http\Request;
require_once 'config.php';
include 'helpers.php';
// composer: monolog/monolog
        "#;
        let file_path = Path::new("test.php");
        let dependencies = processor.extract_dependencies(content, file_path);

        assert_eq!(dependencies.len(), 6);
        assert!(
            dependencies
                .iter()
                .any(|d| d.name == "App\\Http\\Controllers" && d.dependency_type == "namespace")
        );
        assert!(
            dependencies
                .iter()
                .any(|d| d.name == "App\\Models\\User" && d.dependency_type == "use")
        );
        assert!(
            dependencies
                .iter()
                .any(|d| d.name == "Illuminate\\Http\\Request" && d.dependency_type == "use")
        );
        assert!(
            dependencies
                .iter()
                .any(|d| d.name == "config.php" && d.dependency_type == "require_once")
        );
        assert!(
            dependencies
                .iter()
                .any(|d| d.name == "helpers.php" && d.dependency_type == "include")
        );
        assert!(
            dependencies
                .iter()
                .any(|d| d.name == "monolog/monolog" && d.dependency_type == "composer")
        );

        let app_use = dependencies
            .iter()
            .find(|d| d.name == "App\\Models\\User")
            .unwrap();
        assert!(!app_use.is_external);

        let vendor_use = dependencies
            .iter()
            .find(|d| d.name == "Illuminate\\Http\\Request")
            .unwrap();
        assert!(vendor_use.is_external);
    }

    #[test]
    fn test_composer_hash_comment() {
        let processor = PhpProcessor::new();
        let content = r#"<?php
# composer: symfony/polyfill-php80
        "#;

        let deps = processor.extract_dependencies(content, Path::new("composer.php"));
        let composer_dep = deps
            .iter()
            .find(|d| d.dependency_type == "composer")
            .unwrap();
        assert_eq!(composer_dep.name, "symfony/polyfill-php80");
    }

    #[test]
    fn test_grouped_use_dependencies() {
        let processor = PhpProcessor::new();
        let content = r#"<?php
use Foo\{Bar, Baz as Quux}, Other\Thing;
"#;

        let deps = processor.extract_dependencies(content, Path::new("grouped.php"));
        assert!(deps.iter().any(|d| d.name == "Foo\\Bar"));
        assert!(deps.iter().any(|d| d.name == "Foo\\Baz"));
        assert!(deps.iter().any(|d| d.name == "Other\\Thing"));
    }

    #[test]
    fn test_multiline_use_statements_with_aliases() {
        let processor = PhpProcessor::new();
        let content = r#"<?php
use Foo\Bar\{
    Baz,
    Qux as QuxAlias,
    Corge
};
use Another\Thing,
    Another\Other as OtherAlias;
"#;

        let deps = processor.extract_dependencies(content, Path::new("multiline.php"));
        assert!(deps.iter().any(|d| d.name == "Foo\\Bar\\Baz"));
        assert!(deps.iter().any(|d| d.name == "Foo\\Bar\\Qux"));
        assert!(deps.iter().any(|d| d.name == "Foo\\Bar\\Corge"));
        assert!(deps.iter().any(|d| d.name == "Another\\Thing"));
        assert!(deps.iter().any(|d| d.name == "Another\\Other"));
    }

    #[test]
    fn test_parenthesized_require_dependencies() {
        let processor = PhpProcessor::new();
        let content = r#"<?php
require_once(__DIR__ . '/bootstrap.php');
include_once ( 'helpers.php' );
"#;

        let deps = processor.extract_dependencies(content, Path::new("requires.php"));
        assert!(
            deps.iter()
                .any(|d| d.name.contains("__DIR__ . '/bootstrap.php'")
                    && d.dependency_type == "require_once")
        );
        assert!(
            deps.iter()
                .any(|d| d.name.contains("helpers.php") && d.dependency_type == "include_once")
        );
    }

    #[test]
    fn test_is_important_line() {
        let processor = PhpProcessor::new();
        assert!(processor.is_important_line(r"<?php class MyClass {}"));
        assert!(processor.is_important_line(r"   public function myFunction() {}"));
        assert!(processor.is_important_line(r"namespace MyNamespace;"));
        assert!(processor.is_important_line(r"use My\OtherClass;"));
        assert!(processor.is_important_line(r"require 'file.php';"));
        assert!(processor.is_important_line(r"/** Docblock */"));
        assert!(processor.is_important_line(r" * Some description"));
        assert!(processor.is_important_line(r"#[Attribute]"));
        assert!(processor.is_important_line(r"// TODO: Fix this"));
        assert!(!processor.is_important_line(r"$var = 1;"));
        assert!(!processor.is_important_line(r"// Just a comment"));
    }

    #[test]
    fn test_extract_interfaces_class() {
        let processor = PhpProcessor::new();
        let content = r#"<?php
/**
 * This is a test class.
 */
class MyClass extends AnotherClass implements MyInterface {
    /**
     * Constructor.
     * @param string $name
     */
    public function __construct(string $name) {}

    /**
     * A public method.
     * @return int
     */
    public function getCount(): int { return 0; }

    private function privateMethod() {}
}
        "#;
        let file_path = Path::new("test.php");
        let interfaces = processor.extract_interfaces(content, file_path);

        assert_eq!(interfaces.len(), 4);

        // Class
        let class_iface = interfaces.iter().find(|i| i.name == "MyClass").unwrap();
        assert_eq!(class_iface.interface_type, "class");
        assert_eq!(
            class_iface.description.as_ref().unwrap(),
            "This is a test class."
        );

        // Constructor
        let ctor_iface = interfaces.iter().find(|i| i.name == "__construct").unwrap();
        assert_eq!(ctor_iface.interface_type, "method"); // Constructors are methods in PHP
        assert_eq!(ctor_iface.parameters.len(), 1);
        assert_eq!(ctor_iface.parameters[0].name, "name");
        assert_eq!(ctor_iface.parameters[0].param_type, "string");

        // Public method
        let public_method_iface = interfaces.iter().find(|i| i.name == "getCount").unwrap();
        assert_eq!(public_method_iface.interface_type, "method");
        assert_eq!(public_method_iface.return_type.as_ref().unwrap(), "int");
        assert_eq!(
            public_method_iface.description.as_ref().unwrap(),
            "A public method."
        );

        // Private method
        let private_method_iface = interfaces
            .iter()
            .find(|i| i.name == "privateMethod")
            .unwrap();
        assert_eq!(private_method_iface.visibility, "private");
    }

    #[test]
    fn test_extract_interfaces_interface() {
        let processor = PhpProcessor::new();
        let content = r#"<?php
/**
 * This is a test interface.
 */
interface MyInterface {
    public function doSomething();
}
        "#;
        let file_path = Path::new("test.php");
        let interfaces = processor.extract_interfaces(content, file_path);
        assert_eq!(interfaces.len(), 2);

        // Interface
        let interface_iface = interfaces.iter().find(|i| i.name == "MyInterface").unwrap();
        assert_eq!(interface_iface.interface_type, "interface");

        // Interface method
        let do_something_iface = interfaces.iter().find(|i| i.name == "doSomething").unwrap();
        assert_eq!(do_something_iface.interface_type, "method");
        assert_eq!(do_something_iface.visibility, "public");
    }

    #[test]
    fn test_extract_interfaces_trait() {
        let processor = PhpProcessor::new();
        let content = r#"<?php
/**
 * This is a test trait.
 */
trait MyTrait {
    public function traitMethod() {}
}
        "#;
        let file_path = Path::new("test.php");
        let interfaces = processor.extract_interfaces(content, file_path);
        assert_eq!(interfaces.len(), 2);

        // Trait
        let trait_iface = interfaces.iter().find(|i| i.name == "MyTrait").unwrap();
        assert_eq!(trait_iface.interface_type, "trait");

        // Trait method
        let trait_method_iface = interfaces.iter().find(|i| i.name == "traitMethod").unwrap();
        assert_eq!(trait_method_iface.interface_type, "method");
        assert_eq!(trait_method_iface.visibility, "public");
    }

    #[test]
    fn test_extract_interfaces_global_function_and_enum() {
        let processor = PhpProcessor::new();
        let content = r#"<?php
/**
 * This is a global function.
 * @param string $param
 * @return bool
 */
function globalFunction(string $param): bool { return true; }

enum MyEnum {
    case Foo;
    case Bar;
}
        "#;
        let file_path = Path::new("test.php");
        let interfaces = processor.extract_interfaces(content, file_path);

        assert_eq!(interfaces.len(), 2);

        // Global function
        let func_iface = interfaces
            .iter()
            .find(|i| i.name == "globalFunction")
            .unwrap();
        assert_eq!(func_iface.interface_type, "function");
        assert_eq!(func_iface.parameters.len(), 1);
        assert_eq!(func_iface.parameters[0].name, "param");
        assert_eq!(func_iface.parameters[0].param_type, "string");
        assert_eq!(func_iface.return_type.as_ref().unwrap(), "bool");
        assert_eq!(
            func_iface.description.as_ref().unwrap(),
            "This is a global function."
        );

        // Enum
        let enum_iface = interfaces.iter().find(|i| i.name == "MyEnum").unwrap();
        assert_eq!(enum_iface.interface_type, "enum");
    }

    #[test]
    fn test_attribute_description_and_union_param() {
        let processor = PhpProcessor::new();
        let content = r#"<?php
#[Route('/')]
class AttrClass {
    #[Inject]
    public function __construct(private LoggerInterface $logger, Foo|Bar $value) {}
}
"#;
        let interfaces = processor.extract_interfaces(content, Path::new("attr.php"));
        let ctor = interfaces.iter().find(|i| i.name == "__construct").unwrap();

        let description = ctor.description.as_ref().unwrap();
        assert!(description.contains("Attributes: #[Inject]"));
        assert!(ctor.parameters.iter().any(|p| p.param_type == "Foo|Bar"));
    }

    #[test]
    fn test_determine_component_type() {
        let processor = PhpProcessor::new();
        let class_content = "class MyClass {}";
        let interface_content = "interface MyInterface {}";
        let trait_content = "trait MyTrait {}";
        let enum_content = "enum MyEnum {}";
        let file_content = "echo 'hello';";

        assert_eq!(
            processor.determine_component_type(Path::new("test.php"), class_content),
            "php_class"
        );
        assert_eq!(
            processor.determine_component_type(Path::new("test.php"), interface_content),
            "php_interface"
        );
        assert_eq!(
            processor.determine_component_type(Path::new("test.php"), trait_content),
            "php_trait"
        );
        assert_eq!(
            processor.determine_component_type(Path::new("test.php"), enum_content),
            "php_enum"
        );
        assert_eq!(
            processor.determine_component_type(Path::new("test.php"), file_content),
            "php_file"
        );
    }

    #[test]
    fn test_is_internal_namespace() {
        let processor = PhpProcessor::new();
        assert!(processor.is_internal_namespace("app\\MyClass"));
        assert!(processor.is_internal_namespace("src\\MyClass"));
        assert!(processor.is_internal_namespace("App\\MyClass"));
        assert!(!processor.is_internal_namespace("MyVendor\\MyClass"));
    }

    #[test]
    fn test_parse_parameters_with_default_array() {
        let processor = PhpProcessor::new();
        let content = r#"
        class MyClass {
            public function __construct(private LoggerInterface $logger, array $options = ['a', 'b']) {}
        }
"#;
        let interfaces = processor.extract_interfaces(content, Path::new("attr.php"));
        let ctor = interfaces.iter().find(|i| i.name == "__construct").unwrap();

        assert_eq!(ctor.parameters.len(), 2);
        let options_param = ctor
            .parameters
            .iter()
            .find(|p| p.name == "options")
            .unwrap();
        assert!(options_param.is_optional);
        assert_eq!(options_param.param_type, "array");
    }

    #[test]
    fn test_manager_selection() {
        let manager = LanguageProcessorManager::new();
        let php_file = Path::new("example.php");
        let rust_file = Path::new("example.rs");

        let php_processor = manager.get_processor(php_file).unwrap();
        assert_eq!(php_processor.language_name(), "PHP");

        let rust_processor = manager.get_processor(rust_file).unwrap();
        assert_eq!(rust_processor.language_name(), "Rust");
    }
}
