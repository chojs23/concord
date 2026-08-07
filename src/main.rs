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

    #[cfg(all(target_os = "macos", feature = "stream-broadcast"))]
    initialize_macos_display_services();

    // Native media libraries can write directly to stderr and corrupt the
    // Ratatui screen. Capture those diagnostics only for the interactive app;
    // CLI commands above keep their normal terminal output.
    let _stderr_capture = concord::logging::capture_stderr()?;
    let app = App::new();
    app.run().await
}

#[cfg(all(target_os = "macos", feature = "stream-broadcast"))]
fn initialize_macos_display_services() {
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGMainDisplayID() -> u32;
    }

    // Concord is a terminal application, so AppKit does not initialize the
    // process's WindowServer connection for us. Do this on the main thread
    // before ScreenCaptureKit creates window filters on capture workers.
    let _ = unsafe { CGMainDisplayID() };
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
    let (_options, app_warnings) = concord::config::load_options_with_warnings()?;
    let (keymap, keymap_warnings) = concord::config::load_keymap_options_with_warnings()?;
    concord::tui::validate_keymap_options(&keymap)?;
    let (theme, theme_parser_warnings) = concord::config::load_theme_options_with_warnings()?;
    for warning in app_warnings
        .into_iter()
        .chain(keymap_warnings)
        .chain(theme_parser_warnings)
        .chain(concord::tui::theme_options_warnings(&theme))
    {
        println!("warning: {warning}");
    }
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
    fn cli_command_maps_arguments_to_commands() {
        for (args, expected) in [
            (vec!["--version".into()], CliCommand::Version),
            (vec!["--check-config".into()], CliCommand::CheckConfig),
            (vec![], CliCommand::Run),
            (vec!["--unknown".into()], CliCommand::Run),
        ] {
            assert_eq!(cli_command_from_args(args.clone()), expected, "{args:?}");
        }
    }
}
