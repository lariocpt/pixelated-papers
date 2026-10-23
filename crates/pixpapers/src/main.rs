//! pixpapers — forced-pixelation wallpapers for niri & other wayland
//! compositors. CLI + daemon entry point.

mod cli;
mod commands;
mod daemon;
mod ipc;
mod status;

use std::process::ExitCode;

use clap::Parser;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    let cmd = cli::Cli::parse();
    let paths = pixcore::Paths::from_env();
    let cfg = match pixcore::Config::load(&paths.config_file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(1);
        }
    };
    match cli::run(cmd, &paths, &cfg) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(if commands::is_user_error(&e) { 1 } else { 2 })
        }
    }
}
