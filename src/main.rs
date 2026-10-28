mod cli;
mod tui;

use clap::Parser;
use std::process::ExitCode;

fn main() -> ExitCode {
    match cli::run(cli::args::Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
