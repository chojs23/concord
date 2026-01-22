use concord::{App, Result};

#[tokio::main]
async fn main() -> Result<()> {
    install_rustls_crypto_provider();

    let app = App::new();
    app.run().await
}

fn install_rustls_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}
