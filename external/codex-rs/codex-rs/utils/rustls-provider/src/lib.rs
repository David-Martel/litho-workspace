use std::sync::Once;

/// Ensures a process-wide rustls crypto provider is installed.
///
/// rustls cannot auto-select a provider when multiple crypto providers
/// are enabled in the dependency graph.
pub fn ensure_rustls_crypto_provider() {
    static RUSTLS_PROVIDER_INIT: Once = Once::new();
    RUSTLS_PROVIDER_INIT.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}
