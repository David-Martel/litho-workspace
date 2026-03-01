use chrono::Utc;
use std::env;
use std::process::Command;

fn run_git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let txt = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if txt.is_empty() { None } else { Some(txt) }
}

fn env_or(key: &str, fallback: impl FnOnce() -> String) -> String {
    env::var(key)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(fallback)
}

fn main() {
    println!("cargo:rerun-if-env-changed=LITHO_BUILD_TIMESTAMP");
    println!("cargo:rerun-if-env-changed=LITHO_BUILD_GIT_TAG");
    println!("cargo:rerun-if-env-changed=LITHO_BUILD_GIT_SHA");
    println!("cargo:rerun-if-env-changed=LITHO_BUILD_TOKEN");

    let pkg_version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_string());
    let timestamp = env_or("LITHO_BUILD_TIMESTAMP", || {
        Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
    });
    let git_tag = env_or("LITHO_BUILD_GIT_TAG", || {
        run_git(&["describe", "--tags", "--always", "--dirty"])
            .unwrap_or_else(|| "unknown-tag".to_string())
    });
    let git_sha = env_or("LITHO_BUILD_GIT_SHA", || {
        run_git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown-sha".to_string())
    });

    let auto_token = format!("{}-{}-{}", git_tag, git_sha, timestamp.replace(':', ""));
    let token = env_or("LITHO_BUILD_TOKEN", || auto_token);
    let version = format!(
        "{} (git_tag={} git_sha={} build_time_utc={} build_token={})",
        pkg_version, git_tag, git_sha, timestamp, token
    );

    println!("cargo:rustc-env=LITHO_BUILD_TIMESTAMP={timestamp}");
    println!("cargo:rustc-env=LITHO_BUILD_GIT_TAG={git_tag}");
    println!("cargo:rustc-env=LITHO_BUILD_GIT_SHA={git_sha}");
    println!("cargo:rustc-env=LITHO_BUILD_TOKEN={token}");
    println!("cargo:rustc-env=LITHO_BUILD_VERSION={version}");
}
