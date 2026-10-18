// SPDX-License-Identifier: GPL-2.0-or-later
//! `washi`: command-line control for Canon CHMP/IVEC inkjet printers.

mod cli;
mod commands;
mod doctor;
mod firmware;
mod jobs;
mod render;

use std::process::ExitCode;

use clap::Parser;

fn main() -> ExitCode {
    let cli = cli::Cli::parse();
    match commands::run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            if let Some(hint) = e.downcast_ref::<washi_core::Error>().and_then(|e| e.hint()) {
                eprintln!("hint: {hint}");
            }
            ExitCode::FAILURE
        }
    }
}
