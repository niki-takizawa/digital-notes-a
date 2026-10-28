//! CLI dispatch. Entity commands are added in subsequent commits.

pub mod args;
mod asset;
mod card;
mod check;
pub(crate) mod editor;
mod evidence;
mod work;

use crate::tui;
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
        Command::Tui => {
            tui::run(&Repository::open(&root)?)?;
        }
        Command::Asset { command } => {
            asset::run(&Repository::open(&root)?, command)?;
        }
        Command::Card { command } => {
            card::run(&Repository::open(&root)?, command)?;
        }
        Command::Work { command } => {
            work::run(&Repository::open(&root)?, command)?;
        }
        Command::Check { json } => {
            check::run(&Repository::open(&root)?, json)?;
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
