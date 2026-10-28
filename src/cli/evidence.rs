//! Register and inspect exact quotations associated with a Card.

use clap::Subcommand;
use digital_notes::{
    evidence::Resolution,
    model::{AssetId, CardId, EvidenceId},
    repository::Repository,
};
use std::{error::Error, io};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Debug, Subcommand)]
pub enum EvidenceCommand {
    /// Record a quote from a saved Asset into a Card.
    Add {
        card: String,
        #[arg(long)]
        asset: String,
        #[arg(long)]
        quote: String,
        /// Codepoint offset in normalized Asset text (required for ambiguous quotes).
        #[arg(long)]
        start: Option<usize>,
        /// Codepoint offset in Card Markdown for citation marker insertion.
        #[arg(long)]
        marker_offset: Option<usize>,
    },
    /// List Evidence entries and their quoted text.
    List {
        card: String,
        #[arg(long)]
        json: bool,
    },
    /// Display one Evidence entry.
    Show {
        card: String,
        evidence: String,
        #[arg(long)]
        json: bool,
    },
    /// Verify citations against immutable HTML snapshots.
    Verify { card: String },
}

fn bad_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

pub fn run(repo: &Repository, command: EvidenceCommand) -> Result<()> {
    match command {
        EvidenceCommand::Add {
            card,
            asset,
            quote,
            start,
            marker_offset,
        } => {
            let card: CardId = card.parse().map_err(bad_input)?;
            let asset: AssetId = asset.parse().map_err(bad_input)?;
            let loaded = repo.load_card(&card)?;
            let saved = repo.add_card_evidence(
                &card,
                &loaded.revision,
                &asset,
                &quote,
                start,
                marker_offset,
            )?;
            let entry = saved
                .value
                .evidence
                .last()
                .ok_or_else(|| bad_input("Evidence addition yielded an empty list"))?;
            println!("Added Evidence {} to Card {}", entry.id, card);
        }
        EvidenceCommand::List { card, json } => {
            let card: CardId = card.parse().map_err(bad_input)?;
            let entries = repo.load_card(&card)?.value.evidence;
            if json {
                println!("{}", serde_json::to_string_pretty(&entries)?);
            } else {
                for entry in entries {
                    println!(
                        "{}  [{}] {}",
                        entry.id, entry.asset_id, entry.selector.exact
                    );
                }
            }
        }
        EvidenceCommand::Show {
            card,
            evidence,
            json,
        } => {
            let card: CardId = card.parse().map_err(bad_input)?;
            let evidence: EvidenceId = evidence.parse().map_err(bad_input)?;
            let loaded = repo.load_card(&card)?;
            let entry = loaded
                .value
                .evidence
                .iter()
                .find(|entry| entry.id == evidence)
                .ok_or_else(|| {
                    bad_input(format!("Evidence {evidence} does not exist in Card {card}"))
                })?;
            if json {
                println!("{}", serde_json::to_string_pretty(entry)?);
            } else {
                println!(
                    "Evidence: {}\nAsset: {}\nQuote: {}\nStart: {}\nEnd: {}\nPrefix: {}\nSuffix: {}",
                    entry.id,
                    entry.asset_id,
                    entry.selector.exact,
                    entry.position_selector.start,
                    entry.position_selector.end,
                    entry.selector.prefix,
                    entry.selector.suffix,
                );
            }
        }
        EvidenceCommand::Verify { card } => {
            let card: CardId = card.parse().map_err(bad_input)?;
            let statuses = repo.verify_card_evidence(&card)?;
            let mut all_valid = true;
            for (id, status) in statuses {
                let description = match status {
                    Resolution::Verified => "verified".to_owned(),
                    Resolution::Recovered(start) => {
                        format!("recovered at {start} (stored position differs)")
                    }
                    Resolution::Unresolved => "unresolved".to_owned(),
                };
                println!("{id}: {description}");
                if status != Resolution::Verified {
                    all_valid = false;
                }
            }
            if !all_valid {
                return Err(bad_input("one or more Evidence entries require review").into());
            }
        }
    }
    Ok(())
}
