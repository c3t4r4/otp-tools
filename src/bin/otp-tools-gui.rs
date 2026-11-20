#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

use otp_tools::gui::launch_gui;
use otp_tools::model::sanitize_terminal_text;

fn main() -> ExitCode {
    match launch_gui() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!(
                "Erro ao iniciar interface gráfica: {}",
                sanitize_terminal_text(&err)
            );
            ExitCode::FAILURE
        }
    }
}
