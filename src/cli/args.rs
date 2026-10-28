//! Command-line argument definitions, shared by future CLI subcommands.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "digital-notes",
    version,
    about = "Manage local research notes and their sources",
    propagate_version = true
)]
pub struct Cli {
    /// Repository root (defaults to DIGITAL_NOTES_ROOT, then current directory).
    #[arg(long, global = true, env = "DIGITAL_NOTES_ROOT", value_name = "DIR")]
    pub root: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Initialize the repository directories without deleting existing data.
    Init,
    /// Audit repository integrity and saved Evidence.
    Check {
        /// Emit a machine-readable report.
        #[arg(long)]
        json: bool,
    },
    /// Capture and browse Assets.
    Asset {
        #[command(subcommand)]
        command: super::asset::AssetCommand,
    },
    /// Manage Cards.
    Card {
        #[command(subcommand)]
        command: super::card::CardCommand,
    },
    /// Manage Work logs.
    Work {
        #[command(subcommand)]
        command: super::work::WorkCommand,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_global_root_before_or_after_subcommand() {
        let before = Cli::try_parse_from(["digital-notes", "--root", "notes", "init"]).unwrap();
        let after = Cli::try_parse_from(["digital-notes", "init", "--root", "notes"]).unwrap();
        for cli in [before, after] {
            assert_eq!(cli.root.as_deref(), Some(std::path::Path::new("notes")));
            assert!(matches!(cli.command, Command::Init));
        }
    }

    #[test]
    fn rejects_unsupported_subcommands_and_missing_root_value() {
        assert!(Cli::try_parse_from(["digital-notes", "unknown"]).is_err());
        assert!(Cli::try_parse_from(["digital-notes", "--root"]).is_err());
    }
}
