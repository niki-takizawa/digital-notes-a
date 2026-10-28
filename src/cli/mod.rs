//! CLI dispatch. Entity commands are added in subsequent commits.

pub mod args;

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
