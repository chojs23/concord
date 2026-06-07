use concord::{App, Result};

#[tokio::main]
async fn main() -> Result<()> {
    install_rustls_crypto_provider();

    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("concord {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let app = App::new();
    app.run().await
}

fn install_rustls_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}
