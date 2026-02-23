use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{debug, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileNode {
    pub name: String,
    pub path: String,
    pub is_file: bool,
    pub children: Vec<FileNode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub file_path: String,
    pub file_name: String,
    pub title: Option<String>,
    pub matches: Vec<SearchMatch>,
    pub relevance_score: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchMatch {
    pub line_number: usize,
    pub content: String,
    pub highlighted_content: String,
    pub context_before: Option<String>,
    pub context_after: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DocumentTree {
    pub root: FileNode,
    pub file_map: HashMap<String, PathBuf>,
    pub stats: TreeStats,
    pub search_index: HashMap<String, Vec<String>>, // file_path -> lines
    // Populated at startup for O(1) node lookups; consumed by future phases.
    #[allow(dead_code)]
    pub node_map: HashMap<String, FileNode>,
    pub docs_base_canonical: PathBuf,
}

#[derive(Debug, Clone)]
pub struct TreeStats {
    pub total_files: usize,
    pub total_dirs: usize,
    pub total_size: u64,
}

impl DocumentTree {
    /// Create a new document tree from the given directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be read.
    pub fn new(docs_dir: &Path) -> anyhow::Result<Self> {
        let mut file_map = HashMap::new();
        let mut search_index = HashMap::new();
        let mut stats = TreeStats {
            total_files: 0,
            total_dirs: 0,
            total_size: 0,
        };

        // Resolve canonical base path for path-traversal validation.
        // Falls back to the raw path when canonicalize fails (e.g. path does not yet exist).
        let docs_base_canonical = docs_dir
            .canonicalize()
            .unwrap_or_else(|_| docs_dir.to_path_buf());

        debug!("Building document tree from: {}", docs_dir.display());

        // Create a virtual root that contains the children of the actual directory
        let mut children = Vec::new();

        // Read directory contents and sort them
        let mut entries: Vec<_> = std::fs::read_dir(docs_dir)?
            .filter_map(|entry| entry.ok())
            .collect();

        // Sort entries: directories first, then files, both case-insensitively alphabetically.
        crate::utils::sort_entries(&mut entries);

        for entry in entries {
            let path = entry.path();

            // Skip hidden files/dirs and non-markdown files.
            if crate::utils::should_skip(&path) {
                continue;
            }

            match Self::build_tree(
                &path,
                docs_dir,
                &mut file_map,
                &mut search_index,
                &mut stats,
            ) {
                Ok(child) => children.push(child),
                Err(e) => {
                    warn!("Failed to process path {}: {}", path.display(), e);
                    continue;
                }
            }
        }

        // Create virtual root node
        let root = FileNode {
            name: "root".to_string(),
            path: "".to_string(),
            is_file: false,
            children,
            size: None,
            modified: None,
        };

        // Build a flat map of path -> FileNode for O(1) node lookups.
        let mut node_map = HashMap::new();
        collect_nodes(&root, &mut node_map);

        debug!(
            "Document tree built: {} files, {} directories, {} bytes total",
            stats.total_files, stats.total_dirs, stats.total_size
        );

        Ok(DocumentTree {
            root,
            file_map,
            stats,
            search_index,
            node_map,
            docs_base_canonical,
        })
    }

    /// Recursively build the file tree.
    fn build_tree(
        current_path: &Path,
        base_path: &Path,
        file_map: &mut HashMap<String, PathBuf>,
        search_index: &mut HashMap<String, Vec<String>>,
        stats: &mut TreeStats,
    ) -> anyhow::Result<FileNode> {
        let name = current_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let relative_path = current_path
            .strip_prefix(base_path)
            .unwrap_or(current_path)
            .to_string_lossy()
            .replace('\\', "/");

        if current_path.is_file() {
            let metadata = std::fs::metadata(current_path)?;
            let size = metadata.len();

            // Delegate timestamp formatting to utils.
            let modified = metadata
                .modified()
                .ok()
                .and_then(crate::utils::format_system_time);

            if current_path.extension().and_then(|s| s.to_str()) == Some("md") {
                file_map.insert(relative_path.clone(), current_path.to_path_buf());

                // Build search index for this file
                if let Ok(content) = std::fs::read_to_string(current_path) {
                    let lines: Vec<String> = content.lines().map(|line| line.to_string()).collect();
                    search_index.insert(relative_path.clone(), lines);
                }

                stats.total_files += 1;
                stats.total_size += size;
            }

            return Ok(FileNode {
                name,
                path: relative_path,
                is_file: true,
                children: vec![],
                size: Some(size),
                modified,
            });
        }

        stats.total_dirs += 1;
        let mut children = Vec::new();

        // Read directory contents and sort them.
        let mut entries: Vec<_> = std::fs::read_dir(current_path)?
            .filter_map(|entry| entry.ok())
            .collect();

        crate::utils::sort_entries(&mut entries);

        for entry in entries {
            let path = entry.path();

            // Skip hidden files/dirs and non-markdown files.
            if crate::utils::should_skip(&path) {
                continue;
            }

            match Self::build_tree(&path, base_path, file_map, search_index, stats) {
                Ok(child) => children.push(child),
                Err(e) => {
                    warn!("Failed to process path {}: {}", path.display(), e);
                    continue;
                }
            }
        }

        Ok(FileNode {
            name,
            path: relative_path,
            is_file: false,
            children,
            size: None,
            modified: None,
        })
    }

    /// Get the content of a file by its relative path.
    ///
    /// Serves from the in-memory search index when the file is indexed, and
    /// falls back to a disk read for non-indexed files.  A path-traversal check
    /// is applied before every disk read.
    pub fn get_file_content(&self, file_path: &str) -> anyhow::Result<String> {
        // Serve from memory if available (avoids disk I/O for all indexed .md files).
        if let Some(lines) = self.search_index.get(file_path) {
            return Ok(lines.join("\n"));
        }

        // Fall back to disk for non-indexed files (e.g. files added after startup).
        let path = self
            .file_map
            .get(file_path)
            .ok_or_else(|| anyhow::anyhow!("File not found: {}", file_path))?;

        debug!("Reading file from disk (not in index): {}", path.display());

        // Path-traversal guard: resolve the absolute path and verify it is
        // rooted inside the docs directory.
        let canonical = path
            .canonicalize()
            .map_err(|e| anyhow::anyhow!("Failed to resolve path {}: {}", path.display(), e))?;

        if !canonical.starts_with(&self.docs_base_canonical) {
            anyhow::bail!("Path traversal detected: {}", file_path);
        }

        std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read file {}: {}", path.display(), e))
    }

    /// Render markdown content to HTML.
    pub fn render_markdown(&self, content: &str) -> String {
        use pulldown_cmark::{Options, Parser, html};

        let mut options = Options::empty();
        options.insert(Options::ENABLE_TABLES);
        options.insert(Options::ENABLE_FOOTNOTES);
        options.insert(Options::ENABLE_STRIKETHROUGH);
        options.insert(Options::ENABLE_TASKLISTS);
        options.insert(Options::ENABLE_SMART_PUNCTUATION);
        options.insert(Options::ENABLE_HEADING_ATTRIBUTES);

        let parser = Parser::new_ext(content, options);
        let mut html_output = String::new();
        html::push_html(&mut html_output, parser);

        html_output
    }

    /// Get statistics about the document tree.
    pub fn get_stats(&self) -> &TreeStats {
        &self.stats
    }

    /// Advanced search with full-text search and content preview.
    pub fn search_content(&self, query: &str) -> Vec<SearchResult> {
        if query.trim().is_empty() {
            return vec![];
        }

        let query_lower = query.to_lowercase();
        let mut results = Vec::new();

        for (file_path, lines) in &self.search_index {
            let mut matches = Vec::new();
            let mut relevance_score = 0.0;

            // Extract title from first heading
            let title = lines
                .iter()
                .find(|line| line.trim().starts_with('#'))
                .map(|line| line.trim_start_matches('#').trim().to_string());

            // Search through all lines
            for (line_number, line) in lines.iter().enumerate() {
                let line_lower = line.to_lowercase();

                if line_lower.contains(&query_lower) {
                    // Calculate relevance score
                    let mut line_score = 1.0;

                    // Higher score for title matches
                    if line.trim().starts_with('#') {
                        line_score *= 3.0;
                    }

                    // Higher score for exact word matches
                    if line_lower
                        .split_whitespace()
                        .any(|word| word == query_lower)
                    {
                        line_score *= 2.0;
                    }

                    // Higher score for matches at the beginning of the line
                    if line_lower.trim_start().starts_with(&query_lower) {
                        line_score *= 1.5;
                    }

                    relevance_score += line_score;

                    // Create highlighted content
                    let highlighted_content = self.highlight_matches(line, query);

                    // Get context lines
                    let context_before = if line_number > 0 {
                        lines.get(line_number - 1).cloned()
                    } else {
                        None
                    };

                    let context_after = lines.get(line_number + 1).cloned();

                    matches.push(SearchMatch {
                        line_number: line_number + 1, // 1-based line numbers
                        content: line.clone(),
                        highlighted_content,
                        context_before,
                        context_after,
                    });
                }
            }

            // Also check filename matches
            let file_name = file_path.rsplit('/').next().unwrap_or(file_path);
            if file_name.to_lowercase().contains(&query_lower) {
                relevance_score += 2.0; // Bonus for filename matches
            }

            if !matches.is_empty() {
                results.push(SearchResult {
                    file_path: file_path.clone(),
                    file_name: file_name.to_string(),
                    title,
                    matches,
                    relevance_score,
                });
            }
        }

        // Sort by relevance score (descending)
        results.sort_by(|a, b| {
            b.relevance_score
                .partial_cmp(&a.relevance_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Limit results to prevent overwhelming the UI
        results.truncate(50);

        results
    }

    /// Highlight ALL occurrences of `query` in `content` using `<mark>` tags.
    ///
    /// Matching is case-insensitive; the original casing of `content` is
    /// preserved in the output.  Special HTML characters are escaped.
    fn highlight_matches(&self, content: &str, query: &str) -> String {
        let query_lower = query.to_lowercase();
        let content_lower = content.to_lowercase();

        let indices: Vec<usize> = content_lower
            .match_indices(&query_lower)
            .map(|(i, _)| i)
            .collect();

        if indices.is_empty() {
            return escape_html(content);
        }

        let mut result = String::new();
        let mut last_end = 0;
        for &start in &indices {
            let end = start + query.len();
            result.push_str(&escape_html(&content[last_end..start]));
            result.push_str("<mark>");
            result.push_str(&escape_html(&content[start..end]));
            result.push_str("</mark>");
            last_end = end;
        }
        result.push_str(&escape_html(&content[last_end..]));
        result
    }
}

// ---------------------------------------------------------------------------
// Module-level helpers
// ---------------------------------------------------------------------------

/// Recursively collect all file nodes from the tree into a flat map keyed by
/// their relative path.
fn collect_nodes(node: &FileNode, map: &mut HashMap<String, FileNode>) {
    if node.is_file {
        map.insert(node.path.clone(), node.clone());
    }
    for child in &node.children {
        collect_nodes(child, map);
    }
}

/// Escape the characters that have special meaning in HTML.
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_html() {
        assert_eq!(escape_html("a &lt; b &amp; c"), "a &amp;lt; b &amp;amp; c");
        assert_eq!(escape_html("<script>"), "&lt;script&gt;");
        assert_eq!(escape_html("no special"), "no special");
        assert_eq!(escape_html(""), "");
    }

    #[test]
    fn test_document_tree_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let tree = DocumentTree::new(dir.path()).unwrap();
        assert_eq!(tree.stats.total_files, 0);
        assert_eq!(tree.stats.total_dirs, 0);
        assert!(tree.root.children.is_empty());
    }

    #[test]
    fn test_document_tree_with_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("hello.md"), "# Hello\nWorld").unwrap();
        std::fs::write(dir.path().join("ignored.txt"), "not markdown").unwrap();

        let tree = DocumentTree::new(dir.path()).unwrap();
        assert_eq!(tree.stats.total_files, 1);
        assert_eq!(tree.root.children.len(), 1);
        assert_eq!(tree.root.children[0].name, "hello.md");
    }

    #[test]
    fn test_document_tree_with_subdirectory() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("subdir");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("nested.md"), "# Nested").unwrap();

        let tree = DocumentTree::new(dir.path()).unwrap();
        assert_eq!(tree.stats.total_files, 1);
        assert_eq!(tree.stats.total_dirs, 1);
        // subdir should be first child (dirs before files)
        assert!(!tree.root.children[0].is_file);
    }

    #[test]
    fn test_get_file_content_from_memory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.md"), "line1\nline2\nline3").unwrap();

        let tree = DocumentTree::new(dir.path()).unwrap();
        let content = tree.get_file_content("test.md").unwrap();
        assert_eq!(content, "line1\nline2\nline3");
    }

    #[test]
    fn test_get_file_content_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let tree = DocumentTree::new(dir.path()).unwrap();
        assert!(tree.get_file_content("nonexistent.md").is_err());
    }

    #[test]
    fn test_search_content_found() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.md"), "# Title\nfoo bar baz\nqux").unwrap();

        let tree = DocumentTree::new(dir.path()).unwrap();
        let results = tree.search_content("bar");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].matches.len(), 1);
        assert_eq!(results[0].matches[0].line_number, 2); // 1-based
    }

    #[test]
    fn test_search_content_empty_query() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.md"), "hello").unwrap();

        let tree = DocumentTree::new(dir.path()).unwrap();
        let results = tree.search_content("");
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_content_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.md"), "Hello World").unwrap();

        let tree = DocumentTree::new(dir.path()).unwrap();
        let results = tree.search_content("hello");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_highlight_matches_all_occurrences() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("x.md"), "dummy").unwrap();
        let tree = DocumentTree::new(dir.path()).unwrap();

        let result = tree.highlight_matches("foo bar foo baz foo", "foo");
        assert_eq!(
            result,
            "<mark>foo</mark> bar <mark>foo</mark> baz <mark>foo</mark>"
        );
    }

    #[test]
    fn test_highlight_matches_escapes_html() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("x.md"), "dummy").unwrap();
        let tree = DocumentTree::new(dir.path()).unwrap();

        let result = tree.highlight_matches("<b>foo</b> & foo", "foo");
        assert_eq!(
            result,
            "&lt;b&gt;<mark>foo</mark>&lt;/b&gt; &amp; <mark>foo</mark>"
        );
    }

    #[test]
    fn test_highlight_matches_no_match() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("x.md"), "dummy").unwrap();
        let tree = DocumentTree::new(dir.path()).unwrap();

        let result = tree.highlight_matches("no match here", "xyz");
        assert_eq!(result, "no match here");
    }

    #[test]
    fn test_render_markdown() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("x.md"), "dummy").unwrap();
        let tree = DocumentTree::new(dir.path()).unwrap();

        let html = tree.render_markdown("# Hello");
        assert!(html.contains("<h1>"));
        assert!(html.contains("Hello"));
    }

    #[test]
    fn test_render_markdown_table() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("x.md"), "dummy").unwrap();
        let tree = DocumentTree::new(dir.path()).unwrap();

        let html = tree.render_markdown("| a | b |\n|---|---|\n| 1 | 2 |");
        assert!(html.contains("<table>"));
    }

    #[test]
    fn test_node_map_populated() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "content").unwrap();
        std::fs::write(dir.path().join("b.md"), "content").unwrap();

        let tree = DocumentTree::new(dir.path()).unwrap();
        assert_eq!(tree.node_map.len(), 2);
        assert!(tree.node_map.contains_key("a.md"));
        assert!(tree.node_map.contains_key("b.md"));
    }

    #[test]
    fn test_hidden_files_skipped() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".hidden.md"), "secret").unwrap();
        std::fs::write(dir.path().join("visible.md"), "public").unwrap();

        let tree = DocumentTree::new(dir.path()).unwrap();
        assert_eq!(tree.stats.total_files, 1);
        assert_eq!(tree.root.children[0].name, "visible.md");
    }

    #[test]
    fn test_get_stats() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "12345").unwrap();

        let tree = DocumentTree::new(dir.path()).unwrap();
        let stats = tree.get_stats();
        assert_eq!(stats.total_files, 1);
        assert_eq!(stats.total_size, 5);
    }
}
