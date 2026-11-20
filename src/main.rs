use std::process::ExitCode;

use clap::Parser;
use owo_colors::OwoColorize;

use otp_tools::cli::{run, Cli};
use otp_tools::model::sanitize_terminal_text;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!(
                "{} {}",
                "Erro:".bright_red().bold(),
                sanitize_terminal_text(&err.to_string())
            );
            ExitCode::FAILURE
        }
    }
}
