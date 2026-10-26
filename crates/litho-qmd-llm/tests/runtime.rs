use litho_qmd_llm::{AdaptiveLlmEngine, NoopLlmEngine};
use litho_qmd_storage::QmdLlmEngine;

struct EnvGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var(key).ok();
        unsafe { std::env::set_var(key, value) };
        Self { key, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => unsafe { std::env::set_var(self.key, value) },
            None => unsafe { std::env::remove_var(self.key) },
        }
    }
}

#[test]
fn noop_llm_expand_query_returns_expected_variants() {
    let engine = NoopLlmEngine;
    let variants = engine
        .expand_query("Rust Config Perf Bug")
        .expect("noop expand_query should succeed");

    assert!(
        variants
            .iter()
            .any(|variant| variant == "Rust Config Perf Bug")
    );
    assert!(
        variants
            .iter()
            .any(|variant| variant == "rust config perf bug")
    );
    assert!(
        variants
            .iter()
            .any(|variant| variant.contains("configuration"))
    );
    assert!(
        variants
            .iter()
            .any(|variant| variant.contains("performance"))
    );
    assert!(variants.iter().any(|variant| variant.contains("issue")));
}

#[test]
#[serial_test::serial]
fn adaptive_llm_respects_noop_provider_env() {
    let _provider = EnvGuard::set("QMD_LLM_PROVIDER", "noop");
    let _force = EnvGuard::set("QMD_LLM_FORCE_OLLAMA", "true");
    let _threshold = EnvGuard::set("QMD_LLM_OLLAMA_COMPLEXITY_THRESHOLD", "999");

    let engine = AdaptiveLlmEngine::from_env();
    let variants = engine
        .expand_query("How to design the build configuration")
        .expect("adaptive noop mode should succeed");

    assert!(
        variants
            .iter()
            .any(|variant| variant.contains("configuration"))
    );
    assert!(variants.iter().all(|variant| !variant.trim().is_empty()));
}
