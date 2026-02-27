use litho_codex::exec::CodexExecGenerator;
use litho_codex::provider::DocGenerator;
use std::env;
use std::path::PathBuf;

#[tokio::test]
async fn test_codex_readiness_failure() {
    // Force a non-existent binary path
    let generator = CodexExecGenerator {
        binary_path: Some(PathBuf::from("non_existent_binary_xyz_123")),
        ..Default::default()
    };

    let result = generator.validate_readiness().await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("not found") || err.contains("not executable"));
}

#[tokio::test]
async fn test_codex_readiness_with_env() {
    // This test assumes 'cargo' is on the path, we can use it as a dummy binary 
    // that supports '--version' to test the logic if codex isn't built yet.
    // However, it's better to test the real thing if possible.
    
    // We can't easily test success without a real binary, 
    // but we can test that the environment variable is respected.
    unsafe { env::set_var("CODEX_BIN", "non_existent_via_env"); }
    let generator = CodexExecGenerator::default();
    let result = generator.validate_readiness().await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("non_existent_via_env"));
    unsafe { env::remove_var("CODEX_BIN"); }
}
