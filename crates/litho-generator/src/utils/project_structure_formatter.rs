use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::types::project_structure::ProjectStructure;

/// Project structure formatter - Responsible for converting project structure data into tree-string representation
pub struct ProjectStructureFormatter;

impl ProjectStructureFormatter {
    /// Format project structure information as a tree structure
    pub fn format_as_tree(structure: &ProjectStructure) -> String {
        let mut result = format!(
            "### Project Structure Information\nProject Name: {}\nRoot Directory: {}\n\nProject Directory Structure:\n```\n",
            structure.project_name,
            structure.root_path.to_string_lossy()
        );

        // Build path tree, distinguishing files and directories
        let mut tree = PathTree::new();

        // First insert all files (these are confirmed files)
        for file in &structure.files {
            let normalized_path = Self::normalize_path(&file.path);
            tree.insert_file(&normalized_path);
        }

        // Generate tree string
        let tree_output = tree.to_tree_string();
        result.push_str(&tree_output);
        result.push_str("```\n");

        result
    }

    /// Format project directory structure as a simplified directory tree (folders only)
    pub fn format_as_directory_tree(structure: &ProjectStructure) -> String {
        let mut result = format!(
            "### Project Directory Structure\nProject Name: {}\nRoot Directory: {}\n\nDirectory Tree:\n```\n",
            structure.project_name,
            structure.root_path.to_string_lossy()
        );

        // Build directory tree, only including directories
        let mut dir_tree = DirectoryTree::new();

        // Extract directory paths from all file paths
        for file in &structure.files {
            let normalized_path = Self::normalize_path(&file.path);
            if let Some(parent_dir) = normalized_path.parent() {
                dir_tree.insert_directory(parent_dir);
            }
        }

        // Generate directory tree string
        let tree_output = dir_tree.to_tree_string();
        result.push_str(&tree_output);
        result.push_str("```\n");

        result
    }

    /// Normalize path format, remove "./" prefix
    fn normalize_path(path: &Path) -> PathBuf {
        let path_str = path.to_string_lossy();
        if path_str.starts_with("./") {
            PathBuf::from(&path_str[2..])
        } else {
            path.to_path_buf()
        }
    }
}

/// Path tree node
#[derive(Debug)]
struct PathNode {
    name: String,
    children: BTreeMap<String, PathNode>,
}

impl PathNode {
    fn new(name: String) -> Self {
        Self {
            name,
            children: BTreeMap::new(),
        }
    }
}

/// Path tree structure
#[derive(Debug)]
struct PathTree {
    root: PathNode,
}

/// Directory tree node (only directories)
#[derive(Debug)]
struct DirectoryNode {
    name: String,
    children: BTreeMap<String, DirectoryNode>,
}

impl DirectoryNode {
    fn new(name: String) -> Self {
        Self {
            name,
            children: BTreeMap::new(),
        }
    }
}

/// Directory tree structure (only directories)
#[derive(Debug)]
struct DirectoryTree {
    root: DirectoryNode,
}

impl DirectoryTree {
    fn new() -> Self {
        Self {
            root: DirectoryNode::new("".to_string()),
        }
    }

    /// Insert directory path into tree
    fn insert_directory(&mut self, path: &Path) {
        let components: Vec<&str> = path
            .components()
            .filter_map(|c| c.as_os_str().to_str())
            .collect();

        if components.is_empty() {
            return;
        }

        let mut current = &mut self.root;

        for component in components.iter() {
            current
                .children
                .entry(component.to_string())
                .or_insert_with(|| DirectoryNode::new(component.to_string()));

            current = current.children.get_mut(*component).unwrap();
        }
    }

    /// Generate directory tree string representation
    fn to_tree_string(&self) -> String {
        let mut result = String::new();
        self.render_directory_node(&self.root, "", true, &mut result);
        result
    }

    /// Recursively render directory node
    fn render_directory_node(&self, node: &DirectoryNode, prefix: &str, is_last: bool, result: &mut String) {
        if !node.name.is_empty() {
            let connector = if is_last { "└── " } else { "├── " };
            result.push_str(&format!("{}{}{}/\n", prefix, connector, node.name));
        }

        let children: Vec<_> = node.children.values().collect();
        for (i, child) in children.iter().enumerate() {
            let is_last_child = i == children.len() - 1;
            let new_prefix = if node.name.is_empty() {
                prefix.to_string()
            } else if is_last {
                format!("{}    ", prefix)
            } else {
                format!("{}│   ", prefix)
            };

            self.render_directory_node(child, &new_prefix, is_last_child, result);
        }
    }
}

impl PathTree {
    fn new() -> Self {
        Self {
            root: PathNode::new("".to_string()),
        }
    }

    /// Insert file path into tree
    fn insert_file(&mut self, path: &Path) {
        self.insert_path(path);
    }

    /// Insert path into tree
    fn insert_path(&mut self, path: &Path) {
        let components: Vec<&str> = path
            .components()
            .filter_map(|c| c.as_os_str().to_str())
            .collect();

        if components.is_empty() {
            return;
        }

        let mut current = &mut self.root;

        for (_i, component) in components.iter().enumerate() {
            current
                .children
                .entry(component.to_string())
                .or_insert_with(|| PathNode::new(component.to_string()));

            current = current.children.get_mut(*component).unwrap();
        }
    }

    /// Generate tree string representation
    fn to_tree_string(&self) -> String {
        let mut result = String::new();
        self.render_node(&self.root, "", true, &mut result);
        result
    }

    /// Recursively render node
    fn render_node(&self, node: &PathNode, prefix: &str, is_last: bool, result: &mut String) {
        if !node.name.is_empty() {
            let connector = if is_last { "└── " } else { "├── " };
            result.push_str(&format!("{}{}{}\n", prefix, connector, node.name));
        }

        let children: Vec<_> = node.children.values().collect();
        for (i, child) in children.iter().enumerate() {
            let is_last_child = i == children.len() - 1;
            let new_prefix = if node.name.is_empty() {
                prefix.to_string()
            } else if is_last {
                format!("{}    ", prefix)
            } else {
                format!("{}│   ", prefix)
            };

            self.render_node(child, &new_prefix, is_last_child, result);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FileInfo;
    use std::path::PathBuf;

    #[test]
    fn test_format_as_directory_tree() {
        let structure = ProjectStructure {
            project_name: "test_project".to_string(),
            root_path: PathBuf::from("/test"),
            files: vec![
                FileInfo {
                    path: PathBuf::from("src/main.rs"),
                    name: "main.rs".to_string(),
                    size: 100,
                    extension: Some("rs".to_string()),
                    is_core: true,
                    importance_score: 0.8,
                    complexity_score: 0.6,
                    last_modified: Some("2024-01-01".to_string()),
                },
                FileInfo {
                    path: PathBuf::from("src/lib.rs"),
                    name: "lib.rs".to_string(),
                    size: 200,
                    extension: Some("rs".to_string()),
                    is_core: true,
                    importance_score: 0.9,
                    complexity_score: 0.7,
                    last_modified: Some("2024-01-01".to_string()),
                },
                FileInfo {
                    path: PathBuf::from("src/utils/mod.rs"),
                    name: "mod.rs".to_string(),
                    size: 50,
                    extension: Some("rs".to_string()),
                    is_core: false,
                    importance_score: 0.5,
                    complexity_score: 0.3,
                    last_modified: Some("2024-01-01".to_string()),
                },
                FileInfo {
                    path: PathBuf::from("tests/integration_test.rs"),
                    name: "integration_test.rs".to_string(),
                    size: 150,
                    extension: Some("rs".to_string()),
                    is_core: false,
                    importance_score: 0.4,
                    complexity_score: 0.5,
                    last_modified: Some("2024-01-01".to_string()),
                },
                FileInfo {
                    path: PathBuf::from("docs/README.md"),
                    name: "README.md".to_string(),
                    size: 300,
                    extension: Some("md".to_string()),
                    is_core: false,
                    importance_score: 0.6,
                    complexity_score: 0.2,
                    last_modified: Some("2024-01-01".to_string()),
                },
            ],
            directories: vec![], // Add required field
            total_files: 5,
            total_directories: 4,
            file_types: std::collections::HashMap::new(),
            size_distribution: std::collections::HashMap::new(),
        };

        let result = ProjectStructureFormatter::format_as_directory_tree(&structure);
        
        // Check basic format
        assert!(result.contains("### Project Directory Structure"));
        assert!(result.contains("test_project"));
        assert!(result.contains("/test"));
        
        // Check directory structure (should only include directories, not files)
        assert!(result.contains("src/"));
        assert!(result.contains("utils/"));
        assert!(result.contains("tests/"));
        assert!(result.contains("docs/"));
        
        // Ensure filenames are not included
        assert!(!result.contains("main.rs"));
        assert!(!result.contains("lib.rs"));
        assert!(!result.contains("mod.rs"));
        assert!(!result.contains("integration_test.rs"));
        assert!(!result.contains("README.md"));
        
        println!("Directory tree output:\n{}", result);
    }

    #[test]
    fn test_directory_tree_structure() {
        let mut dir_tree = DirectoryTree::new();
        
        // Insert some directory paths
        dir_tree.insert_directory(&PathBuf::from("src"));
        dir_tree.insert_directory(&PathBuf::from("src/utils"));
        dir_tree.insert_directory(&PathBuf::from("tests"));
        dir_tree.insert_directory(&PathBuf::from("docs"));
        
        let result = dir_tree.to_tree_string();
        
        // Check tree structure
        assert!(result.contains("src/"));
        assert!(result.contains("utils/"));
        assert!(result.contains("tests/"));
        assert!(result.contains("docs/"));
        
        // Check tree connectors
        assert!(result.contains("├──") || result.contains("└──"));
        
        println!("Tree structure:\n{}", result);
    }
}