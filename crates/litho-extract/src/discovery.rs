use ignore::WalkBuilder;
use litho_core::types::Language;
use std::path::{Path, PathBuf};

/// A source file discovered during project walking.
pub struct DiscoveredFile {
    /// Absolute path to the file.
    pub path: PathBuf,
    /// Detected programming language.
    pub language: Language,
    /// File size in bytes.
    pub size: u64,
}

/// Walk `root` respecting `.gitignore` and the given exclusion lists.
///
/// Files whose size is 0 or exceeds `max_file_size` are omitted.
pub fn discover_files(
    root: &Path,
    excluded_dirs: &[String],
    excluded_extensions: &[String],
    max_file_size: u64,
) -> Vec<DiscoveredFile> {
    let mut files = Vec::new();

    let walker = WalkBuilder::new(root).hidden(true).git_ignore(true).build();

    for entry in walker.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        // Skip excluded directories.
        if excluded_dirs.iter().any(|dir| {
            path.components()
                .any(|c| c.as_os_str().to_str().map(|s| s == dir).unwrap_or(false))
        }) {
            continue;
        }

        // Skip excluded extensions.
        if let Some(ext) = path.extension().and_then(|e| e.to_str())
            && excluded_extensions.iter().any(|e| e == ext)
        {
            continue;
        }

        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if size == 0 || size > max_file_size {
            continue;
        }

        let language = detect_language(path);
        files.push(DiscoveredFile {
            path: path.to_owned(),
            language,
            size,
        });
    }

    files
}

/// Map a file's extension to a [`Language`] variant.
pub fn detect_language(path: &Path) -> Language {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => Language::Rust,
        Some("ts" | "tsx") => Language::TypeScript,
        Some("js" | "jsx" | "mjs") => Language::JavaScript,
        Some("py") => Language::Python,
        Some("cs") => Language::CSharp,
        Some("ps1" | "psm1" | "psd1") => Language::PowerShell,
        Some("java") => Language::Java,
        Some("go") => Language::Go,
        Some("rb") => Language::Ruby,
        Some("cpp" | "cc" | "cxx" | "hpp") => Language::Cpp,
        Some("c" | "h") => Language::C,
        _ => Language::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn detects_rust() {
        assert_eq!(detect_language(Path::new("src/lib.rs")), Language::Rust);
    }

    #[test]
    fn detects_typescript() {
        assert_eq!(detect_language(Path::new("index.ts")), Language::TypeScript);
        assert_eq!(detect_language(Path::new("app.tsx")), Language::TypeScript);
    }

    #[test]
    fn detects_python() {
        assert_eq!(
            detect_language(Path::new("script.py")),
            Language::Python
        );
    }

    #[test]
    fn detects_unknown() {
        assert_eq!(
            detect_language(Path::new("README.md")),
            Language::Unknown
        );
    }
}
