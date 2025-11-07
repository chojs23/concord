use discord_client_terminal::{App, Config, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::from_env()?;
    let app = App::new(config);
    app.run().await
}
