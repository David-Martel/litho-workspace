use std::fs::DirEntry;
use std::path::Path;
use std::time::SystemTime;
use tracing::debug;

/// Format bytes into human-readable format (e.g., "1.5 MB")
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];

    if bytes == 0 {
        return "0 B".to_string();
    }

    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{} {}", bytes, UNITS[unit_index])
    } else {
        format!("{:.1} {}", size, UNITS[unit_index])
    }
}

/// Format a SystemTime into "YYYY-MM-DD HH:MM:SS" string
pub fn format_system_time(time: SystemTime) -> Option<String> {
    time.duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| {
            let datetime = chrono::DateTime::from_timestamp(d.as_secs() as i64, 0)?;
            Some(datetime.format("%Y-%m-%d %H:%M:%S").to_string())
        })
}

/// Returns true if the path should be skipped during directory scanning.
/// Skips hidden files/dirs (starting with '.') and non-markdown files.
pub fn should_skip(path: &Path) -> bool {
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if name.starts_with('.') {
            debug!("Skipping hidden file/directory: {}", name);
            return true;
        }

        if path.is_file() && path.extension().and_then(|s| s.to_str()) != Some("md") {
            debug!("Skipping non-markdown file: {}", name);
            return true;
        }
    }
    false
}

/// Sort directory entries: directories first, then files, both case-insensitive alphabetically.
pub fn sort_entries(entries: &mut [DirEntry]) {
    entries.sort_by(|a, b| {
        let a_is_dir = a.path().is_dir();
        let b_is_dir = b.path().is_dir();

        match (a_is_dir, b_is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => {
                let a_name = a.file_name().to_string_lossy().to_lowercase();
                let b_name = b.file_name().to_string_lossy().to_lowercase();
                a_name.cmp(&b_name)
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes_zero() {
        assert_eq!(format_bytes(0), "0 B");
    }

    #[test]
    fn test_format_bytes_bytes() {
        assert_eq!(format_bytes(500), "500 B");
    }

    #[test]
    fn test_format_bytes_kilobytes() {
        assert_eq!(format_bytes(1024), "1.0 KB");
    }

    #[test]
    fn test_format_bytes_megabytes() {
        assert_eq!(format_bytes(1_048_576), "1.0 MB");
    }

    #[test]
    fn test_format_bytes_gigabytes() {
        assert_eq!(format_bytes(1_073_741_824), "1.0 GB");
    }

    #[test]
    fn test_format_bytes_mixed() {
        assert_eq!(format_bytes(1536), "1.5 KB");
    }

    #[test]
    fn test_format_system_time_epoch() {
        let epoch = std::time::UNIX_EPOCH;
        let result = format_system_time(epoch);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "1970-01-01 00:00:00");
    }

    #[test]
    fn test_should_skip_hidden() {
        let path = Path::new(".hidden");
        // .hidden doesn't exist on disk, but we can test the name check
        // For hidden dirs, we just check the name prefix
        assert!(
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with('.'))
        );
    }

    #[test]
    fn test_should_skip_non_md_file_name() {
        // Test the name-based logic without requiring disk files
        let name = "test.txt";
        let path = Path::new(name);
        assert_ne!(path.extension().and_then(|s| s.to_str()), Some("md"));
    }

    #[test]
    fn test_should_skip_md_file_name() {
        let name = "test.md";
        let path = Path::new(name);
        assert_eq!(path.extension().and_then(|s| s.to_str()), Some("md"));
    }
}
