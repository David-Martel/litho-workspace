use std::env;

/// Compile-time build metadata embedded by crate-level `build.rs`.
#[derive(Debug, Clone)]
pub struct BuildStamp {
    pub pkg_version: &'static str,
    pub git_tag: &'static str,
    pub git_sha: &'static str,
    pub build_time_utc: &'static str,
    pub build_token: &'static str,
    pub version_string: &'static str,
}

/// Helper: const-compatible fallback for `option_env!().unwrap_or()`.
/// `Option::unwrap_or` is not yet const-stable (rust#143874).
macro_rules! option_env_or {
    ($key:expr, $fallback:expr) => {
        match option_env!($key) {
            Some(v) => v,
            None => $fallback,
        }
    };
}

impl BuildStamp {
    pub const fn current() -> Self {
        Self {
            pkg_version: env!("CARGO_PKG_VERSION"),
            git_tag: option_env_or!("LITHO_BUILD_GIT_TAG", "unknown-tag"),
            git_sha: option_env_or!("LITHO_BUILD_GIT_SHA", "unknown-sha"),
            build_time_utc: option_env_or!("LITHO_BUILD_TIMESTAMP", "unknown-time"),
            build_token: option_env_or!("LITHO_BUILD_TOKEN", "unknown-token"),
            version_string: option_env_or!("LITHO_BUILD_VERSION", env!("CARGO_PKG_VERSION")),
        }
    }
}

/// Build token compiled into this binary/crate.
pub fn build_token() -> &'static str {
    BuildStamp::current().build_token
}

/// Rich version string compiled into this binary/crate.
pub fn version_string() -> &'static str {
    BuildStamp::current().version_string
}

/// Validate that the runtime expected token matches the compile-time token.
///
/// Intended for CI/build-pipeline sync checks:
/// set `LITHO_EXPECT_BUILD_TOKEN` before running tests/binaries.
pub fn assert_expected_token_sync() -> Result<(), String> {
    let expected = match env::var("LITHO_EXPECT_BUILD_TOKEN") {
        Ok(v) if !v.trim().is_empty() => v,
        _ => return Ok(()),
    };

    let actual = build_token();
    if actual != expected {
        return Err(format!(
            "build token mismatch: expected='{expected}', actual='{actual}'"
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_stamp_is_non_empty() {
        let stamp = BuildStamp::current();
        assert!(!stamp.pkg_version.is_empty());
        assert!(!stamp.version_string.is_empty());
        assert!(!stamp.build_token.is_empty());
    }

    #[test]
    fn expected_token_check_is_noop_without_env() {
        // By contract this returns Ok when no runtime expectation is configured.
        assert!(assert_expected_token_sync().is_ok());
    }

    #[test]
    fn expected_token_matches_when_provided() {
        if let Ok(expected) = env::var("LITHO_EXPECT_BUILD_TOKEN")
            && !expected.trim().is_empty()
        {
            assert_eq!(
                build_token(),
                expected,
                "compiled build token differs from pipeline token"
            );
        }
    }
}
