use discord_client_terminal::{App, Config, Result};

#[tokio::main]
async fn main() -> Result<()> {
    install_rustls_crypto_provider();

    let config = Config::from_env()?;
    let app = App::new(config);
    app.run().await
}

fn install_rustls_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}
