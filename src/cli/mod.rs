//! CLI dispatch. Entity commands are added in subsequent commits.

pub mod args;
mod card;
mod editor;
mod work;

use args::{Cli, Command};
use digital_notes::repository::Repository;
use std::error::Error;

pub fn run(cli: Cli) -> Result<(), Box<dyn Error>> {
    // clap resolves the command-line option before DIGITAL_NOTES_ROOT.
    let root = match cli.root {
        Some(root) => root,
        None => std::env::current_dir()?,
    };

    match cli.command {
        Command::Card { command } => {
            card::run(&Repository::open(&root)?, command)?;
        }
        Command::Work { command } => {
            work::run(&Repository::open(&root)?, command)?;
        }
        Command::Init => {
            let repository = Repository::init(&root)?;
            println!(
                "Initialized digital-notes repository: {}",
                repository.root().display()
            );
        }
    }
    Ok(())
}
