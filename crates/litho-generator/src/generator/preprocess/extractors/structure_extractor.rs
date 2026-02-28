use crate::generator::context::GeneratorContext;
use crate::generator::preprocess::agents::code_purpose_analyze::CodePurposeEnhancer;
use crate::generator::preprocess::extractors::language_processors::LanguageProcessorManager;
use crate::types::code::{CodeDossier, CodePurpose, CodePurposeMapper};
use crate::types::project_structure::ProjectStructure;
use crate::types::{DirectoryInfo, FileInfo};
use crate::utils::file_utils::{is_binary_file_path, is_test_directory, is_test_file};
use crate::utils::sources::read_code_source;
use anyhow::Result;
use futures::future::BoxFuture;
use futures::stream::{self, StreamExt};
use std::collections::HashMap;
use std::fs::Metadata;
use std::path::PathBuf;

/// Project structure extractor
pub struct StructureExtractor {
    language_processor: LanguageProcessorManager,
    code_purpose_enhancer: CodePurposeEnhancer,
    context: GeneratorContext,
}

impl StructureExtractor {
    pub fn new(context: GeneratorContext) -> Self {
        Self {
            language_processor: LanguageProcessorManager::new(),
            code_purpose_enhancer: CodePurposeEnhancer::new(),
            context,
        }
    }

    /// Extract project structure
    pub async fn extract_structure(&self, project_path: &PathBuf) -> Result<ProjectStructure> {
        let cache_key = format!("structure_{}", project_path.display());

        // Execute structure extraction
        let structure = self.extract_structure_impl(project_path).await?;

        // Cache results, structure cache is only used for observation records
        self.context
            .cache_manager
            .write()
            .await
            .set("structure", &cache_key, &structure)
            .await?;

        Ok(structure)
    }

    async fn extract_structure_impl(&self, project_path: &PathBuf) -> Result<ProjectStructure> {
        let mut directories = Vec::new();
        let mut files = Vec::new();
        let mut file_types = HashMap::new();
        let mut size_distribution = HashMap::new();

        // Scan directory, extract internal directory and file structure and basic file information
        self.scan_directory(
            project_path,
            project_path,
            &mut directories,
            &mut files,
            &mut file_types,
            &mut size_distribution,
            0,
            self.context.config.max_depth.into(),
        )
        .await?;

        // Calculate importance scores
        self.calculate_importance_scores(&mut files, &mut directories);

        let project_name = self.context.config.get_project_name();

        Ok(ProjectStructure {
            project_name,
            root_path: project_path.clone(),
            total_files: files.len(),
            total_directories: directories.len(),
            directories,
            files,
            file_types,
            size_distribution,
        })
    }

    fn scan_directory<'a>(
        &'a self,
        current_path: &'a PathBuf,
        root_path: &'a PathBuf,
        directories: &'a mut Vec<DirectoryInfo>,
        files: &'a mut Vec<FileInfo>,
        file_types: &'a mut HashMap<String, usize>,
        size_distribution: &'a mut HashMap<String, usize>,
        current_depth: usize,
        max_depth: usize,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if current_depth > max_depth {
                return Ok(());
            }

            let mut entries = tokio::fs::read_dir(current_path).await?;
            let mut dir_file_count = 0;
            let mut dir_subdirectory_count = 0;
            let mut dir_total_size = 0;

            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                let file_type = entry.file_type().await?;

                if file_type.is_file() {
                    // Check if this file should be ignored
                    if !self.should_ignore_file(&path) {
                        if let Ok(metadata) = tokio::fs::metadata(&path).await {
                            let file_info = self.create_file_info(&path, root_path, &metadata)?;

                            // Update statistics
                            if let Some(ext) = &file_info.extension {
                                *file_types.entry(ext.clone()).or_insert(0) += 1;
                            }

                            let size_category = self.categorize_file_size(file_info.size);
                            *size_distribution.entry(size_category).or_insert(0) += 1;

                            dir_file_count += 1;
                            dir_total_size += file_info.size;

                            files.push(file_info);
                        }
                    }
                } else if file_type.is_dir() {
                    let dir_name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();

                    // Compute the path relative to the project root so that
                    // multi-component exclusion patterns such as
                    // "facts/reference_templates" can be matched against the
                    // real directory subtree, not just the bare leaf name.
                    let relative_path = path
                        .strip_prefix(root_path)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .replace('\\', "/");

                    // Skip hidden directories and commonly ignored directories
                    if !self.should_ignore_directory(&dir_name, &relative_path) {
                        dir_subdirectory_count += 1;

                        // Recursively scan subdirectories
                        self.scan_directory(
                            &path,
                            root_path,
                            directories,
                            files,
                            file_types,
                            size_distribution,
                            current_depth + 1,
                            max_depth,
                        )
                        .await?;
                    }
                }
            }

            // Create directory information
            if current_path != root_path {
                let dir_info = DirectoryInfo {
                    path: current_path.clone(),
                    name: current_path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                    file_count: dir_file_count,
                    subdirectory_count: dir_subdirectory_count,
                    total_size: dir_total_size,
                    importance_score: 0.0, // Calculate later
                };
                directories.push(dir_info);
            }

            Ok(())
        })
    }

    fn create_file_info(
        &self,
        path: &PathBuf,
        root_path: &PathBuf,
        metadata: &Metadata,
    ) -> Result<FileInfo> {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|s| s.to_string());

        let relative_path = path.strip_prefix(root_path).unwrap_or(path).to_path_buf();

        let last_modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs().to_string());

        Ok(FileInfo {
            path: relative_path,
            name,
            size: metadata.len(),
            extension,
            is_core: false,        // Calculate later
            importance_score: 0.0, // Calculate later
            complexity_score: 0.0, // Calculate later
            last_modified,
        })
    }

    fn categorize_file_size(&self, size: u64) -> String {
        match size {
            0..=1024 => "tiny".to_string(),
            1025..=10240 => "small".to_string(),
            10241..=102400 => "medium".to_string(),
            102401..=1048576 => "large".to_string(),
            _ => "huge".to_string(),
        }
    }

    /// Returns `true` when the directory should be excluded from scanning.
    ///
    /// Two kinds of `excluded_dirs` entries are supported:
    ///
    /// * **Simple name** — e.g. `"external"`, `"target"`. The bare directory
    ///   name is compared case-insensitively against every path component.
    /// * **Relative sub-path** — e.g. `"facts/reference_templates"`. The
    ///   relative path of the directory from the project root (always using
    ///   forward slashes) is checked for a case-insensitive prefix match so
    ///   that the entire subtree is excluded.
    fn should_ignore_directory(&self, dir_name: &str, relative_path: &str) -> bool {
        let config = &self.context.config;
        let dir_name_lower = dir_name.to_lowercase();
        let relative_path_lower = relative_path.to_lowercase();

        // Check excluded directories configured in Config.
        for excluded_dir in &config.excluded_dirs {
            let excluded_lower = excluded_dir.to_lowercase();

            if excluded_lower.contains('/') {
                // Multi-component pattern: match against the relative path.
                // We accept both an exact match and a prefix match (so that
                // parent directories of the excluded sub-path are not blocked
                // prematurely, but the excluded dir itself and all children are
                // skipped).
                if relative_path_lower == excluded_lower
                    || relative_path_lower.starts_with(&format!("{}/", excluded_lower))
                {
                    return true;
                }
            } else {
                // Simple name pattern: match against the bare directory name.
                if dir_name_lower == excluded_lower {
                    return true;
                }
            }
        }

        // Check if it's a test directory (if not including test files)
        if !config.include_tests && is_test_directory(dir_name) {
            return true;
        }

        // Check hidden directories
        if !config.include_hidden && dir_name.starts_with('.') {
            return true;
        }

        false
    }

    fn should_ignore_file(&self, path: &PathBuf) -> bool {
        let config = &self.context.config;
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();

        let _path_str = path.to_string_lossy().to_lowercase();

        // Check excluded files
        for excluded_file in &config.excluded_files {
            if excluded_file.contains('*') {
                // Simple wildcard matching
                let pattern = excluded_file.replace('*', "");
                if file_name.contains(&pattern.to_lowercase()) {
                    return true;
                }
            } else if file_name == excluded_file.to_lowercase() {
                return true;
            }
        }

        // Check excluded extensions
        if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
            if config
                .excluded_extensions
                .contains(&extension.to_lowercase())
            {
                return true;
            }
        }

        // Check included extensions (if specified)
        if !config.included_extensions.is_empty() {
            if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
                if !config
                    .included_extensions
                    .contains(&extension.to_lowercase())
                {
                    return true;
                }
            } else {
                return true; // No extension and include list is specified
            }
        }

        // Check test files (if not including test files)
        if !config.include_tests && is_test_file(path) {
            return true;
        }

        // Check hidden files
        if !config.include_hidden && file_name.starts_with('.') {
            return true;
        }

        // Check file size
        if let Ok(metadata) = std::fs::metadata(path) {
            if metadata.len() > config.max_file_size {
                return true;
            }
        }

        // Check binary files
        if is_binary_file_path(path) {
            return true;
        }

        false
    }

    fn calculate_importance_scores(
        &self,
        files: &mut [FileInfo],
        directories: &mut [DirectoryInfo],
    ) {
        // Calculate file importance scores
        for file in files.iter_mut() {
            let mut score: f64 = 0.0;

            // Weight based on file location
            let path_str = file.path.to_string_lossy().to_lowercase();
            if path_str.contains("src") || path_str.contains("lib") {
                score += 0.3;
            }
            if path_str.contains("main") || path_str.contains("index") {
                score += 0.2;
            }
            if path_str.contains("config") || path_str.contains("setup") {
                score += 0.1;
            }

            // Weight based on file size
            if file.size > 1024 && file.size < 50 * 1024 {
                score += 0.2;
            }

            // Weight based on file type
            if let Some(ext) = &file.extension {
                match ext.as_str() {
                    // Main programming languages
                    "rs" | "py" | "java" | "kt" | "cpp" | "c" | "go" | "rb" | "php" | "m"
                    | "swift" | "dart" | "cs" => score += 0.3,
                    // React special files
                    "jsx" | "tsx" => score += 0.3,
                    // JavaScript/TypeScript ecosystem
                    "js" | "ts" | "mjs" | "cjs" => score += 0.3,
                    // Frontend framework files
                    "vue" | "svelte" => score += 0.3,
                    // Mini App
                    "wxml" | "ttml" | "ksml" => score += 0.3,
                    // SQL and database files
                    "sql" | "sqlproj" => score += 0.25,
                    // .NET project files
                    "csproj" | "sln" => score += 0.2,
                    // Configuration files
                    "toml" | "yaml" | "yml" | "json" | "xml" | "ini" | "env" => score += 0.1,
                    // Build and package management files
                    "gradle" | "pom" => score += 0.15,
                    "package" => score += 0.15,
                    "lock" => score += 0.05,
                    // Style files
                    "css" | "scss" | "sass" | "less" | "styl" | "wxss" => score += 0.1,
                    // Template files
                    "html" | "htm" | "hbs" | "mustache" | "ejs" => score += 0.1,
                    _ => {}
                }
            }

            // Bonus for database-related paths
            let path_str = file.path.to_string_lossy().to_lowercase();
            if path_str.contains("database")
                || path_str.contains("schema")
                || path_str.contains("migrations")
            {
                score += 0.15;
            }

            // Bonus for tools/ directory (Python projects often use tools/ as main source)
            if path_str.contains("/tools/") || path_str.contains("\\tools\\") {
                score += 0.15;
            }

            file.importance_score = score.min(1.0);
            file.is_core = score >= 0.5;
        }

        // Calculate directory importance scores
        for dir in directories.iter_mut() {
            let mut score: f64 = 0.0;

            // Based on directory name
            let name_lower = dir.name.to_lowercase();
            if name_lower == "src" || name_lower == "lib" {
                score += 0.4;
            }
            if name_lower.contains("core") || name_lower.contains("main") {
                score += 0.3;
            }

            // Based on file count
            if dir.file_count > 5 {
                score += 0.2;
            }

            // Based on subdirectory count
            if dir.subdirectory_count > 2 {
                score += 0.1;
            }

            dir.importance_score = score.min(1.0);
        }
    }

    /// Identify core files
    pub async fn identify_core_codes(
        &self,
        structure: &ProjectStructure,
    ) -> Result<Vec<CodeDossier>> {
        // Filter core files based on importance score
        let mut core_files: Vec<_> = structure.files.iter().filter(|f| f.is_core).collect();

        // Sort by importance score in descending order, ensuring the most important components are processed first
        core_files.sort_by(|a, b| {
            b.importance_score
                .partial_cmp(&a.importance_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let max_parallel = self.context.config.llm.max_parallels.max(1);
        let indexed_results =
            stream::iter(core_files.into_iter().enumerate().map(|(idx, file)| {
                let file = file.clone();
                async move {
                    let code_purpose = self.determine_code_purpose(&file).await;

                    // Extract interface information
                    let interfaces = self
                        .extract_file_interfaces(&file)
                        .await
                        .unwrap_or_default();
                    let interface_names: Vec<String> =
                        interfaces.iter().map(|i| i.name.clone()).collect();

                    // Extract core code summary
                    let source_summary = read_code_source(
                        &self.language_processor,
                        &structure.root_path,
                        &file.path,
                        &self.context.config.target_language,
                    );

                    (
                        idx,
                        CodeDossier {
                            name: file.name.clone(),
                            file_path: file.path.clone(),
                            source_summary,
                            code_purpose,
                            importance_score: file.importance_score,
                            description: None, // Filled later through LLM analysis
                            functions: Vec::new(), // Filled later through code analysis
                            interfaces: interface_names, // Interface names extracted from code analysis
                        },
                    )
                }
            }))
            .buffer_unordered(max_parallel)
            .collect::<Vec<_>>()
            .await;

        let mut indexed_results = indexed_results;
        indexed_results.sort_by_key(|(idx, _)| *idx);
        let core_codes: Vec<CodeDossier> = indexed_results
            .into_iter()
            .map(|(_, dossier)| dossier)
            .collect();

        Ok(core_codes)
    }

    async fn determine_code_purpose(&self, file: &FileInfo) -> CodePurpose {
        // Read file content
        let file_content = tokio::fs::read_to_string(&file.path).await.ok();

        // Use enhanced component type analyzer
        match self
            .code_purpose_enhancer
            .execute(
                &self.context,
                &file.path,
                &file.name,
                file_content.unwrap_or_default().as_str(),
            )
            .await
        {
            Ok(code_purpose) => code_purpose,
            Err(_) => {
                // Fallback to basic rule mapping
                CodePurposeMapper::map_by_path_and_name(&file.path.to_string_lossy(), &file.name)
            }
        }
    }

    /// Extract file interface information
    async fn extract_file_interfaces(
        &self,
        file: &FileInfo,
    ) -> Result<Vec<crate::types::code::InterfaceInfo>> {
        // Build complete file path
        let full_path = if file.path.is_absolute() {
            file.path.clone()
        } else {
            file.path.clone()
        };

        // Try to read file content
        if let Ok(content) = tokio::fs::read_to_string(&full_path).await {
            // Use language processor to extract interfaces
            let interfaces = self
                .language_processor
                .extract_interfaces(&full_path, &content);

            return Ok(interfaces);
        }

        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cache::CacheManager,
        config::Config,
        generator::context::GeneratorContext,
        llm::client::LLMClient,
        memory::Memory,
    };
    use std::sync::Arc;
    use tokio::sync::RwLock;

    /// Build a `StructureExtractor` wired to a default config so we can call
    /// its private methods via the public `StructureExtractor` API.
    fn make_extractor() -> StructureExtractor {
        let config = Config::default();
        let llm_client = LLMClient::new(config.clone())
            .expect("LLMClient::new should not fail with default config");
        let cache_manager = Arc::new(RwLock::new(CacheManager::new(
            config.cache.clone(),
            config.target_language.clone(),
        )));
        let memory = Arc::new(RwLock::new(Memory::new()));
        let context = GeneratorContext {
            llm_client,
            config,
            cache_manager,
            memory,
        };
        StructureExtractor::new(context)
    }

    // Helper: build a bare `FileInfo` without going through the filesystem.
    fn make_file_info(path: &str, size: u64, extension: Option<&str>) -> FileInfo {
        FileInfo {
            path: PathBuf::from(path),
            name: PathBuf::from(path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            size,
            extension: extension.map(|s| s.to_string()),
            is_core: false,
            importance_score: 0.0,
            complexity_score: 0.0,
            last_modified: None,
        }
    }

    fn make_dir_info(name: &str, file_count: usize, subdir_count: usize) -> DirectoryInfo {
        DirectoryInfo {
            path: PathBuf::from(name),
            name: name.to_string(),
            file_count,
            subdirectory_count: subdir_count,
            total_size: 0,
            importance_score: 0.0,
        }
    }

    // ── categorize_file_size ──────────────────────────────────────────────────

    #[test]
    fn test_categorize_file_size_tiny() {
        let ex = make_extractor();
        assert_eq!(ex.categorize_file_size(0), "tiny");
        assert_eq!(ex.categorize_file_size(512), "tiny");
        assert_eq!(ex.categorize_file_size(1024), "tiny");
    }

    #[test]
    fn test_categorize_file_size_small() {
        let ex = make_extractor();
        assert_eq!(ex.categorize_file_size(1025), "small");
        assert_eq!(ex.categorize_file_size(5000), "small");
        assert_eq!(ex.categorize_file_size(10240), "small");
    }

    #[test]
    fn test_categorize_file_size_medium() {
        let ex = make_extractor();
        assert_eq!(ex.categorize_file_size(10241), "medium");
        assert_eq!(ex.categorize_file_size(50_000), "medium");
        assert_eq!(ex.categorize_file_size(102400), "medium");
    }

    #[test]
    fn test_categorize_file_size_large() {
        let ex = make_extractor();
        assert_eq!(ex.categorize_file_size(102401), "large");
        assert_eq!(ex.categorize_file_size(500_000), "large");
        assert_eq!(ex.categorize_file_size(1_048_576), "large");
    }

    #[test]
    fn test_categorize_file_size_huge() {
        let ex = make_extractor();
        assert_eq!(ex.categorize_file_size(1_048_577), "huge");
        assert_eq!(ex.categorize_file_size(u64::MAX), "huge");
    }

    // ── calculate_importance_scores / is_core threshold ──────────────────────

    #[test]
    fn test_is_core_true_when_score_exactly_0_5() {
        let ex = make_extractor();
        // A Rust source file in src/ with a "good" size (>1KB, <50KB) and in "main" path
        // scores: 0.3 (src) + 0.3 (rs ext) + 0.2 (size 1025..50KB) = 0.8 → is_core
        // We want to test the boundary at exactly 0.5.
        // Craft a file whose score is exactly 0.5: "config" path bonus (0.1) + "rs" ext (0.3) + size bonus (0.2) = 0.6
        // Simpler: path with "src" (0.3) + ext "rs" (0.3) and size 0 (no size bonus) = 0.6 → still is_core
        // To land on exactly 0.5: "config" (0.1) + "rs" (0.3) + size bonus (0.2) = 0.6 — still >= 0.5
        // Use ext "toml" (0.1) + path "config" (0.1) + size bonus (0.2) = 0.4 → NOT is_core
        // ext "toml" (0.1) + path "src/config" (0.3 src + 0.1 config = 0.4) + size bonus 0.2 = 0.7 → is_core
        //
        // For an exact 0.5 test use ext "rs" (0.3) + no other bonuses except size bonus (0.2) = 0.5
        let mut files = vec![make_file_info(
            "standalone.rs",
            5000, // size >1024 and <50*1024 → +0.2
            Some("rs"),
        )];
        // score = 0.3 (ext rs) + 0.2 (size) = 0.5 → is_core must be true (>= 0.5)
        let mut dirs = vec![];
        ex.calculate_importance_scores(&mut files, &mut dirs);
        assert!(
            files[0].is_core,
            "File with score 0.5 must be core (>= 0.5 threshold), score={}",
            files[0].importance_score
        );
        assert!(
            (files[0].importance_score - 0.5).abs() < 1e-9,
            "Expected score 0.5, got {}",
            files[0].importance_score
        );
    }

    #[test]
    fn test_is_core_false_when_score_below_0_5() {
        let ex = make_extractor();
        // ext "toml" (0.1) + size 0 (tiny, no bonus) = 0.1 → NOT is_core
        let mut files = vec![make_file_info("settings.toml", 100, Some("toml"))];
        let mut dirs = vec![];
        ex.calculate_importance_scores(&mut files, &mut dirs);
        assert!(
            !files[0].is_core,
            "File with score < 0.5 must NOT be core, score={}",
            files[0].importance_score
        );
    }

    #[test]
    fn test_tools_path_bonus_pushes_file_to_core() {
        let ex = make_extractor();
        // ext "py" (0.3) + size bonus 0 + tools/ path bonus (0.15) = 0.45 → NOT core without src
        // add size bonus (0.2): 0.3 + 0.15 + 0.2 = 0.65 → is_core
        let mut files = vec![make_file_info(
            "tools/runner.py",
            5000, // >1024, <50KB → +0.2
            Some("py"),
        )];
        let mut dirs = vec![];
        ex.calculate_importance_scores(&mut files, &mut dirs);

        // tools/ bonus must be applied
        let score = files[0].importance_score;
        assert!(
            score >= 0.5,
            "File in tools/ with py ext + size bonus should be core, score={}",
            score
        );
        assert!(files[0].is_core, "tools/ file must be is_core, score={}", score);
    }

    #[test]
    fn test_tools_path_bonus_not_applied_without_tools_prefix() {
        let ex = make_extractor();
        // Same file NOT in tools/
        let mut files = vec![make_file_info("scripts/runner.py", 5000, Some("py"))];
        let mut dirs = vec![];
        ex.calculate_importance_scores(&mut files, &mut dirs);
        // ext "py" (0.3) + size (0.2) = 0.5 → is_core but NOT because of tools/ bonus
        // Check the score does not include the +0.15 tools bonus
        let score = files[0].importance_score;
        // If tools/ bonus were wrongly applied, score would be 0.65
        // Without tools/ bonus, score = 0.3 + 0.2 = 0.5
        assert!(
            (score - 0.5).abs() < 1e-9,
            "Non-tools/ script.py should score exactly 0.5, got {}",
            score
        );
    }

    #[test]
    fn test_importance_score_capped_at_1_0() {
        let ex = make_extractor();
        // src/main.rs with size bonus: 0.3(src) + 0.2(main) + 0.3(rs) + 0.2(size) = 1.0 exactly
        // Add database path: would push it past 1.0 without capping
        let mut files = vec![make_file_info(
            "src/main/database/schema.rs",
            5000,
            Some("rs"),
        )];
        let mut dirs = vec![];
        ex.calculate_importance_scores(&mut files, &mut dirs);
        assert!(
            files[0].importance_score <= 1.0,
            "Score must be capped at 1.0, got {}",
            files[0].importance_score
        );
    }

    #[test]
    fn test_directory_importance_src_or_lib() {
        let ex = make_extractor();
        let mut files = vec![];
        let mut dirs = vec![make_dir_info("src", 0, 0)];
        ex.calculate_importance_scores(&mut files, &mut dirs);
        assert!(
            dirs[0].importance_score >= 0.4,
            "src directory must score >= 0.4, got {}",
            dirs[0].importance_score
        );
    }

    #[test]
    fn test_directory_importance_score_capped_at_1_0() {
        let ex = make_extractor();
        // "src" (0.4) + name contains "core" (0.3) + file_count>5 (0.2) + subdir_count>2 (0.1) = 1.0
        let mut files = vec![];
        let mut dirs = vec![make_dir_info("src-core", 10, 5)];
        ex.calculate_importance_scores(&mut files, &mut dirs);
        assert!(
            dirs[0].importance_score <= 1.0,
            "Dir score must be capped at 1.0, got {}",
            dirs[0].importance_score
        );
    }

    // ── should_ignore_directory (via default config) ──────────────────────────

    #[test]
    fn test_should_ignore_excluded_dir_target() {
        let ex = make_extractor();
        // "target" is in the default excluded_dirs list
        assert!(
            ex.should_ignore_directory("target", "target"),
            "target/ must be ignored by default"
        );
    }

    #[test]
    fn test_should_ignore_hidden_directory_by_default() {
        let ex = make_extractor();
        // include_hidden = false by default
        assert!(
            ex.should_ignore_directory(".git", ".git"),
            ".git must be ignored (hidden)"
        );
        assert!(
            ex.should_ignore_directory(".mydir", ".mydir"),
            "hidden dirs must be ignored"
        );
    }

    #[test]
    fn test_should_not_ignore_normal_source_directory() {
        let ex = make_extractor();
        assert!(
            !ex.should_ignore_directory("src", "src"),
            "src/ must not be ignored"
        );
        assert!(
            !ex.should_ignore_directory("lib", "lib"),
            "lib/ must not be ignored"
        );
    }

    #[test]
    fn test_should_ignore_multi_component_excluded_path() {
        // Simulate a config that excludes a sub-path (don't need a default extractor here)
        let mut config = Config::default();
        config.excluded_dirs.push("facts/reference_templates".to_string());

        let llm_client = LLMClient::new(config.clone()).unwrap();
        let cache_manager = Arc::new(RwLock::new(CacheManager::new(
            config.cache.clone(),
            config.target_language.clone(),
        )));
        let memory = Arc::new(RwLock::new(Memory::new()));
        let context = GeneratorContext { llm_client, config, cache_manager, memory };
        let ex2 = StructureExtractor::new(context);

        // The leaf name is "reference_templates", the relative path is the multi-component pattern
        assert!(
            ex2.should_ignore_directory("reference_templates", "facts/reference_templates"),
            "Multi-component excluded path must be ignored"
        );
        // The leaf name alone should NOT be ignored at a different path
        assert!(
            !ex2.should_ignore_directory("reference_templates", "other/reference_templates"),
            "Same leaf at different path must NOT be ignored"
        );
    }
}
