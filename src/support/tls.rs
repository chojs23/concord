use std::sync::{Arc, OnceLock};

use rustls_platform_verifier::ConfigVerifierExt;

/// Reuses one platform verifier so every WebSocket connection follows the
/// same certificate trust policy as reqwest.
pub(crate) fn websocket_connector() -> Result<tokio_tungstenite::Connector, String> {
    static CONFIG: OnceLock<Result<Arc<rustls::ClientConfig>, String>> = OnceLock::new();

    CONFIG
        .get_or_init(|| {
            rustls::ClientConfig::with_platform_verifier()
                .map(Arc::new)
                .map_err(|error| format!("could not configure platform TLS verifier: {error}"))
        })
        .clone()
        .map(tokio_tungstenite::Connector::Rustls)
}
