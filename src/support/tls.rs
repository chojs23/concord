use std::sync::{Arc, OnceLock};

use rustls_platform_verifier::ConfigVerifierExt;
use wreq::tls::trust::CertStore;

/// Builds an HTTP client with OS roots and no service-specific headers or cookies.
/// Returns an error if the native trust store cannot be loaded.
pub(crate) fn http_client_builder() -> Result<wreq::ClientBuilder, String> {
    Ok(wreq::Client::builder()
        .tls_cert_store(native_cert_store()?)
        .tls_cert_verification(true)
        .tls_verify_hostname(true)
        .redirect(wreq::redirect::Policy::limited(10)))
}

fn native_cert_store() -> Result<CertStore, String> {
    static STORE: OnceLock<Result<CertStore, String>> = OnceLock::new();
    STORE
        .get_or_init(|| {
            let roots = rustls_native_certs::load_native_certs();
            if !roots.errors.is_empty() || roots.certs.is_empty() {
                return Err(format!("load native TLS roots: {:?}", roots.errors));
            }
            // BoringSSL validates the chain and hostname against OS roots. This is
            // not the OS verifier and does not reproduce all OS revocation policies.
            CertStore::from_der_certs(roots.certs.iter().map(|cert| cert.as_ref()))
                .map_err(|error| format!("build native TLS root store: {error}"))
        })
        .clone()
}

/// Reuses the OS verifier for rustls WebSocket connections.
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
