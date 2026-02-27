use std::path::Path;

/// Check if a file is a test file
pub fn is_test_file(path: &Path) -> bool {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();

    let path_str = path.to_string_lossy().to_lowercase();

    // Path-based checks (support different path separators)
    if path_str.contains("/test/")
        || path_str.contains("\\test\\")
        || path_str.contains("/tests/")
        || path_str.contains("\\tests\\")
        || path_str.contains("/__tests__/")
        || path_str.contains("\\__tests__\\")
        || path_str.contains("/spec/")
        || path_str.contains("\\spec\\")
        || path_str.contains("/specs/")
        || path_str.contains("\\specs\\")
        || path_str.starts_with("test/")
        || path_str.starts_with("test\\")
        || path_str.starts_with("tests/")
        || path_str.starts_with("tests\\")
        || path_str.starts_with("__tests__/")
        || path_str.starts_with("__tests__\\")
        || path_str.starts_with("spec/")
        || path_str.starts_with("spec\\")
        || path_str.starts_with("specs/")
        || path_str.starts_with("specs\\")
    {
        return true;
    }

    // Filename-based checks
    // Python test files
    if file_name.starts_with("test_") || file_name.ends_with("_test.py") {
        return true;
    }

    // JavaScript/TypeScript test files
    if file_name.ends_with(".test.js")
        || file_name.ends_with(".spec.js")
        || file_name.ends_with(".test.ts")
        || file_name.ends_with(".spec.ts")
        || file_name.ends_with(".test.jsx")
        || file_name.ends_with(".spec.jsx")
        || file_name.ends_with(".test.tsx")
        || file_name.ends_with(".spec.tsx")
    {
        return true;
    }

    // Java test files
    if file_name.ends_with("test.java") || file_name.ends_with("tests.java") {
        return true;
    }

    // C# test files
    if file_name.ends_with("test.cs")
        || file_name.ends_with("tests.cs")
        || file_name.ends_with(".test.cs")
        || file_name.ends_with(".tests.cs")
    {
        return true;
    }

    // Rust test files
    if file_name.ends_with("_test.rs") || file_name.ends_with("_tests.rs") {
        return true;
    }

    // Go test files
    if file_name.ends_with("_test.go") {
        return true;
    }

    // C/C++ test files
    if file_name.ends_with("_test.c")
        || file_name.ends_with("_test.cpp")
        || file_name.ends_with("_test.cc")
        || file_name.ends_with("test.c")
        || file_name.ends_with("test.cpp")
        || file_name.ends_with("test.cc")
    {
        return true;
    }

    // Generic test filename patterns
    if file_name.contains("test")
        && (file_name.starts_with("test")
            || file_name.ends_with("test")
            || file_name.contains("_test_")
            || file_name.contains(".test.")
            || file_name.contains("-test-")
            || file_name.contains("-test.")
            || file_name.contains(".spec.")
            || file_name.contains("_spec_")
            || file_name.contains("-spec-")
            || file_name.contains("-spec."))
    {
        return true;
    }

    false
}

/// Check if a directory is a test directory
pub fn is_test_directory(dir_name: &str) -> bool {
    let name_lower = dir_name.to_lowercase();

    // Common test directory names
    matches!(
        name_lower.as_str(),
        "test"
            | "tests"
            | "__tests__"
            | "spec"
            | "specs"
            | "testing"
            | "test_data"
            | "testdata"
            | "fixtures"
            | "e2e"
            | "integration"
            | "unit"
            | "acceptance"
    ) || name_lower.ends_with("_test")
        || name_lower.ends_with("_tests")
        || name_lower.ends_with("-test")
        || name_lower.ends_with("-tests")
}

/// Check if a file path is a binary file
#[allow(dead_code)]
pub fn is_binary_file_path(path: &Path) -> bool {
    if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
        let ext_lower = extension.to_lowercase();
        matches!(
            ext_lower.as_str(),
            // Image files
            "jpg" | "jpeg" | "png" | "gif" | "bmp" | "ico" | "svg" | "webp" |
            // Audio files
            "mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a" |
            // Video files
            "mp4" | "avi" | "mkv" | "mov" | "wmv" | "flv" | "webm" |
            // Compressed files
            "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" |
            // Executable files
            "exe" | "dll" | "so" | "dylib" | "bin" |
            // Document files
            "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" |
            // Font files
            "ttf" | "otf" | "woff" | "woff2" |
            // Other binary files
            "db" | "sqlite" | "sqlite3" | "dat" | "cache" |
            "archive"
        )
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // -----------------------------------------------------------------------
    // is_test_file
    // -----------------------------------------------------------------------

    #[test]
    fn rust_test_file_by_name_suffix() {
        assert!(is_test_file(Path::new("my_module_test.rs")));
        assert!(is_test_file(Path::new("my_module_tests.rs")));
    }

    #[test]
    fn rust_source_file_not_test() {
        assert!(!is_test_file(Path::new("main.rs")));
        assert!(!is_test_file(Path::new("config.rs")));
        assert!(!is_test_file(Path::new("utils.rs")));
    }

    #[test]
    fn js_spec_files_are_tests() {
        assert!(is_test_file(Path::new("app.test.js")));
        assert!(is_test_file(Path::new("app.spec.js")));
        assert!(is_test_file(Path::new("component.test.ts")));
        assert!(is_test_file(Path::new("component.spec.tsx")));
    }

    #[test]
    fn python_test_file_prefix() {
        assert!(is_test_file(Path::new("test_main.py")));
        assert!(is_test_file(Path::new("test_config.py")));
    }

    #[test]
    fn python_test_file_suffix() {
        assert!(is_test_file(Path::new("main_test.py")));
    }

    #[test]
    fn path_in_tests_directory_is_test() {
        assert!(is_test_file(Path::new("src/tests/helper.rs")));
        assert!(is_test_file(Path::new("src/test/utils.py")));
    }

    #[test]
    fn path_in_spec_directory_is_test() {
        assert!(is_test_file(Path::new("spec/models/user.js")));
        assert!(is_test_file(Path::new("specs/api.ts")));
    }

    #[test]
    fn go_test_file() {
        assert!(is_test_file(Path::new("handler_test.go")));
    }

    #[test]
    fn java_test_file() {
        assert!(is_test_file(Path::new("UserServiceTest.java")));
        assert!(is_test_file(Path::new("IntegrationTests.java")));
    }

    #[test]
    fn csharp_test_file() {
        assert!(is_test_file(Path::new("UserServiceTest.cs")));
        assert!(is_test_file(Path::new("Helpers.Tests.cs")));
    }

    #[test]
    fn normal_source_file_not_test() {
        assert!(!is_test_file(Path::new("src/lib.rs")));
        assert!(!is_test_file(Path::new("handler.go")));
        assert!(!is_test_file(Path::new("UserService.java")));
    }

    // -----------------------------------------------------------------------
    // is_test_directory
    // -----------------------------------------------------------------------

    #[test]
    fn test_dir_names() {
        assert!(is_test_directory("test"));
        assert!(is_test_directory("tests"));
        assert!(is_test_directory("__tests__"));
        assert!(is_test_directory("spec"));
        assert!(is_test_directory("specs"));
        assert!(is_test_directory("e2e"));
        assert!(is_test_directory("integration"));
        assert!(is_test_directory("unit"));
    }

    #[test]
    fn test_dir_case_insensitive() {
        assert!(is_test_directory("Tests"));
        assert!(is_test_directory("TESTS"));
        assert!(is_test_directory("Spec"));
    }

    #[test]
    fn test_dir_suffix_patterns() {
        assert!(is_test_directory("api_test"));
        assert!(is_test_directory("api_tests"));
        assert!(is_test_directory("api-test"));
        assert!(is_test_directory("api-tests"));
    }

    #[test]
    fn non_test_directory_names() {
        assert!(!is_test_directory("src"));
        assert!(!is_test_directory("lib"));
        assert!(!is_test_directory("target"));
        assert!(!is_test_directory("node_modules"));
    }

    // -----------------------------------------------------------------------
    // is_binary_file_path
    // -----------------------------------------------------------------------

    #[test]
    fn image_extensions_are_binary() {
        for ext in &["jpg", "jpeg", "png", "gif", "bmp", "ico"] {
            let filename = format!("image.{}", ext);
            let path = Path::new(&filename);
            assert!(
                is_binary_file_path(path),
                "expected binary: {}",
                path.display()
            );
        }
    }

    #[test]
    fn executable_extensions_are_binary() {
        assert!(is_binary_file_path(Path::new("app.exe")));
        assert!(is_binary_file_path(Path::new("lib.dll")));
        assert!(is_binary_file_path(Path::new("libfoo.so")));
    }

    #[test]
    fn archive_extensions_are_binary() {
        assert!(is_binary_file_path(Path::new("archive.zip")));
        assert!(is_binary_file_path(Path::new("data.tar")));
        assert!(is_binary_file_path(Path::new("docs.pdf")));
    }

    #[test]
    fn source_extensions_not_binary() {
        assert!(!is_binary_file_path(Path::new("main.rs")));
        assert!(!is_binary_file_path(Path::new("app.py")));
        assert!(!is_binary_file_path(Path::new("index.ts")));
        assert!(!is_binary_file_path(Path::new("Config.cs")));
    }

    #[test]
    fn no_extension_not_binary() {
        assert!(!is_binary_file_path(Path::new("Makefile")));
        assert!(!is_binary_file_path(Path::new("Dockerfile")));
    }
}
