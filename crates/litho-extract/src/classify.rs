use litho_core::types::FileClassification;
use std::path::Path;

/// Infer the coarse role of a file based on its path and content signals.
///
/// `has_main` should be `true` when the caller detected a top-level `main`
/// function (or equivalent entry-point symbol) inside the file.
pub fn classify_file(path: &Path, has_main: bool) -> FileClassification {
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    let path_str = path.to_string_lossy().to_lowercase();

    // Test files take priority.
    if name.ends_with("_test")
        || name.ends_with(".test")
        || name.starts_with("test_")
        || path_str.contains("/tests/")
        || path_str.contains("\\tests\\")
        || path_str.contains("/test/")
        || path_str.contains("\\test\\")
        || name == "tests"
        || name == "test"
    {
        return FileClassification::Test;
    }

    // Explicit entry-points.
    if has_main || name == "main" || name == "program" || name == "app" {
        return FileClassification::EntryPoint;
    }

    // Configuration files.
    if matches!(
        ext.as_str(),
        "toml" | "json" | "yaml" | "yml" | "ini" | "env"
    ) || name.contains("config")
        || name.contains("settings")
    {
        return FileClassification::Config;
    }

    // Build scripts.
    if name == "build"
        || name == "makefile"
        || name == "cargo"
        || ext == "cmake"
        || name == "justfile"
    {
        return FileClassification::BuildScript;
    }

    // Documentation.
    if matches!(ext.as_str(), "md" | "rst" | "txt" | "adoc") {
        return FileClassification::Documentation;
    }

    // Module root / index files.
    if matches!(name.as_str(), "mod" | "index" | "lib" | "__init__") {
        return FileClassification::Module;
    }

    // Regular source files.
    if matches!(
        ext.as_str(),
        "rs" | "ts" | "js" | "py" | "cs" | "java" | "go" | "rb"
    ) {
        return FileClassification::Library;
    }

    FileClassification::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn classifies_test_file() {
        assert_eq!(
            classify_file(Path::new("src/tests/foo.rs"), false),
            FileClassification::Test
        );
        assert_eq!(
            classify_file(Path::new("test_bar.py"), false),
            FileClassification::Test
        );
    }

    #[test]
    fn classifies_entry_point_by_name() {
        assert_eq!(
            classify_file(Path::new("src/main.rs"), false),
            FileClassification::EntryPoint
        );
    }

    #[test]
    fn classifies_entry_point_by_has_main() {
        assert_eq!(
            classify_file(Path::new("src/cli.rs"), true),
            FileClassification::EntryPoint
        );
    }

    #[test]
    fn classifies_config() {
        assert_eq!(
            classify_file(Path::new("Cargo.toml"), false),
            FileClassification::Config
        );
    }

    #[test]
    fn classifies_library() {
        assert_eq!(
            classify_file(Path::new("src/utils.rs"), false),
            FileClassification::Library
        );
    }
}
