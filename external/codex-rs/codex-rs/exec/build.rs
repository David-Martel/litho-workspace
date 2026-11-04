fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    // Match the standalone workspace's Windows stack policy when built from an outer workspace.
    match std::env::var("CARGO_CFG_TARGET_ENV").as_deref() {
        Ok("msvc") => println!("cargo:rustc-link-arg-bin=codex-exec=/STACK:8388608"),
        Ok("gnu") => println!("cargo:rustc-link-arg-bin=codex-exec=-Wl,--stack,8388608"),
        _ => {}
    }
}
