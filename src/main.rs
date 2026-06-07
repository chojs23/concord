use std::ffi::OsString;

use concord::{App, Result};

#[derive(Debug, PartialEq, Eq)]
enum CliCommand {
    Run,
    Version,
    CheckConfig,
}

#[tokio::main]
async fn main() -> Result<()> {
    install_rustls_crypto_provider();

    match cli_command_from_args(std::env::args_os().skip(1)) {
        CliCommand::Run => {}
        CliCommand::Version => {
            println!("concord {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        CliCommand::CheckConfig => {
            check_config()?;
            return Ok(());
        }
    }

    let app = App::new();
    app.run().await
}

fn cli_command_from_args(args: impl IntoIterator<Item = OsString>) -> CliCommand {
    match args
        .into_iter()
        .next()
        .and_then(|arg| arg.into_string().ok())
    {
        Some(arg) if arg == "--version" => CliCommand::Version,
        Some(arg) if arg == "--check-config" => CliCommand::CheckConfig,
        _ => CliCommand::Run,
    }
}

fn check_config() -> Result<()> {
    let _options = concord::config::load_options()?;
    let keymap = concord::config::load_keymap_options()?;
    concord::tui::validate_keymap_options(&keymap)?;
    println!("concord config OK");
    Ok(())
}

fn install_rustls_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

#[cfg(test)]
mod tests {
    use super::{CliCommand, cli_command_from_args};

    #[test]
    fn cli_command_detects_version() {
        assert_eq!(
            cli_command_from_args(["--version".into()]),
            CliCommand::Version
        );
    }

    #[test]
    fn cli_command_detects_config_check() {
        assert_eq!(
            cli_command_from_args(["--check-config".into()]),
            CliCommand::CheckConfig
        );
    }

    #[test]
    fn cli_command_defaults_to_app_run() {
        assert_eq!(cli_command_from_args([]), CliCommand::Run);
        assert_eq!(cli_command_from_args(["--unknown".into()]), CliCommand::Run);
    }
}
