//! Card CLI: create notes within a Work, browse, edit and manage source links.
//! Evidence extraction/selection is intentionally deferred to the Evidence CLI.
use super::editor;
use clap::Subcommand;
use digital_notes::{
    model::{
        AssetId, Card, CardEntityType, CardId, SchemaVersion, SnakeCase, Title, UtcTimestamp,
        WorkId,
    },
    repository::Repository,
};
use std::{collections::HashMap, error::Error, fs, io, path::PathBuf};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Debug, Subcommand)]
pub enum CardCommand {
    /// Create a Card within an existing Work.
    New {
        #[arg(long)]
        title: String,
        /// Work responsible for creating the Card (immutable).
        #[arg(long)]
        work: String,
        #[arg(long = "type", default_value = "analysis")]
        kind: String,
        #[arg(long)]
        body_file: Option<PathBuf>,
        #[arg(long = "source-work")]
        source_works: Vec<String>,
        #[arg(long = "source-asset")]
        source_assets: Vec<String>,
        #[arg(long = "source-card")]
        source_cards: Vec<String>,
        #[arg(long)]
        id_only: bool,
    },
    /// List Cards with titles first; show IDs for duplicate titles.
    List {
        #[arg(long)]
        json: bool,
        #[arg(long)]
        ids: bool,
    },
    /// Show a Card by its ID.
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Edit Markdown with $VISUAL/$EDITOR or read it from a UTF-8 file.
    Edit {
        id: String,
        #[arg(long)]
        body_file: Option<PathBuf>,
    },
    /// Change a Card's title or kind.
    Set {
        id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long = "type")]
        kind: Option<String>,
    },
    /// Add/remove Work, Asset or Card source references.
    Source {
        #[command(subcommand)]
        command: CardSourceCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum CardSourceCommand {
    Work {
        #[command(subcommand)]
        command: SourceAction,
    },
    Asset {
        #[command(subcommand)]
        command: SourceAction,
    },
    Card {
        #[command(subcommand)]
        command: SourceAction,
    },
}

#[derive(Debug, Subcommand)]
pub enum SourceAction {
    Add { id: String, source: String },
    Remove { id: String, source: String },
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn parse_id<T>(raw: &str) -> Result<T>
where
    T: std::str::FromStr<Err = String>,
{
    Ok(raw.parse().map_err(invalid)?)
}

fn parse_many<T>(raw: Vec<String>) -> Result<Vec<T>>
where
    T: std::str::FromStr<Err = String>,
{
    raw.iter().map(|id| parse_id(id)).collect()
}

fn parse_title(raw: String) -> Result<Title> {
    Ok(raw.parse().map_err(invalid)?)
}

fn parse_kind(raw: String) -> Result<SnakeCase> {
    Ok(raw.parse().map_err(invalid)?)
}

/// Check the source before taking the write lock in Repository::create/update_card.
/// Local invariants (duplicates, self-references, Evidence coherence) remain
/// enforced by the Repository itself.
fn verify_sources(repo: &Repository, card: &Card) -> Result<()> {
    repo.load_work(&card.created_by_work_id)?;
    for id in &card.source_work_ids {
        repo.load_work(id)?;
    }
    for id in &card.source_asset_ids {
        repo.load_asset(id)?;
    }
    for id in &card.source_card_ids {
        repo.load_card(id)?;
    }
    Ok(())
}

pub fn run(repo: &Repository, command: CardCommand) -> Result<()> {
    match command {
        CardCommand::New {
            title,
            work,
            kind,
            body_file,
            source_works,
            source_assets,
            source_cards,
            id_only,
        } => {
            let created_by_work_id: WorkId = parse_id(&work)?;
            let body = match body_file {
                Some(path) => fs::read_to_string(path)?,
                None => String::new(),
            };
            let now = UtcTimestamp::now();
            let card = Card {
                schema_version: SchemaVersion,
                id: uuid::Uuid::new_v4().to_string().parse().map_err(invalid)?,
                entity_type: CardEntityType::Card,
                title: parse_title(title)?,
                created: now,
                updated: now,
                card_type: parse_kind(kind)?,
                created_by_work_id,
                source_work_ids: parse_many(source_works)?,
                source_asset_ids: parse_many(source_assets)?,
                source_card_ids: parse_many(source_cards)?,
                evidence: Vec::new(),
                body,
            };
            verify_sources(repo, &card)?;
            let saved = repo.create_card(card)?.value;
            if id_only {
                println!("{}", saved.id);
            } else {
                println!("Created: {}\nID: {}", saved.title, saved.id);
            }
        }
        CardCommand::List { json, ids } => {
            let mut cards = repo.list_cards()?;
            cards.sort_by(|a, b| a.title.as_str().cmp(b.title.as_str()).then(a.id.cmp(&b.id)));
            if json {
                println!("{}", serde_json::to_string_pretty(&cards)?);
            } else {
                let mut counts = HashMap::<&str, usize>::new();
                for card in &cards {
                    *counts.entry(card.title.as_str()).or_default() += 1;
                }
                for card in &cards {
                    if ids || counts[card.title.as_str()] > 1 {
                        println!("{} [{}]  ({})", card.title, card.id, card.card_type);
                    } else {
                        println!("{}  ({})", card.title, card.card_type);
                    }
                }
            }
        }
        CardCommand::Show { id, json } => {
            let card = repo.load_card(&parse_id::<CardId>(&id)?)?.value;
            if json {
                println!("{}", serde_json::to_string_pretty(&card)?);
            } else {
                println!(
                    "{}\nID: {}\nType: {}\nCreated: {}\nUpdated: {}",
                    card.title, card.id, card.card_type, card.created, card.updated
                );
                let creator = repo
                    .load_work(&card.created_by_work_id)
                    .map(|loaded| loaded.value.title.to_string())
                    .unwrap_or_else(|_| "[missing Work]".to_owned());
                println!("Created by: {creator} [{}]", card.created_by_work_id);
                if !card.source_work_ids.is_empty() {
                    println!("Source Works:");
                    for id in &card.source_work_ids {
                        let title = repo
                            .load_work(id)
                            .map(|l| l.value.title.to_string())
                            .unwrap_or_else(|_| "[missing Work]".to_owned());
                        println!("  - {title} [{id}]");
                    }
                }
                if !card.source_asset_ids.is_empty() {
                    println!("Source Assets:");
                    for id in &card.source_asset_ids {
                        let title = repo
                            .load_asset(id)
                            .map(|l| l.value.title.to_string())
                            .unwrap_or_else(|_| "[missing Asset]".to_owned());
                        println!("  - {title} [{id}]");
                    }
                }
                if !card.source_card_ids.is_empty() {
                    println!("Source Cards:");
                    for id in &card.source_card_ids {
                        let title = repo
                            .load_card(id)
                            .map(|l| l.value.title.to_string())
                            .unwrap_or_else(|_| "[missing Card]".to_owned());
                        println!("  - {title} [{id}]");
                    }
                }
                if !card.evidence.is_empty() {
                    println!("Evidence:");
                    for evidence in &card.evidence {
                        println!("  - {} (Asset {})", evidence.id, evidence.asset_id);
                    }
                }
                if !card.body.is_empty() {
                    println!("\n{}", card.body);
                }
            }
        }
        CardCommand::Edit { id, body_file } => {
            let id = parse_id::<CardId>(&id)?;
            let loaded = repo.load_card(&id)?;
            if let Some(path) = body_file {
                let body = fs::read_to_string(path)?;
                let saved = repo.update_card(&id, &loaded.revision, |card| card.body = body)?;
                println!("Saved: {}", saved.value.title);
            } else {
                let edited = editor::edit_markdown(&loaded.value.body)?;
                let result = repo.update_card(&id, &loaded.revision, |card| {
                    card.body = edited.text.clone();
                });
                match result {
                    Ok(saved) => println!("Saved: {}", saved.value.title),
                    Err(error) => {
                        let path = edited.preserve()?;
                        return Err(io::Error::other(format!(
                            "{error}; edited Markdown preserved at {}",
                            path.display()
                        ))
                        .into());
                    }
                }
            }
        }
        CardCommand::Set { id, title, kind } => {
            if title.is_none() && kind.is_none() {
                return Err(invalid("specify --title and/or --type").into());
            }
            let id = parse_id::<CardId>(&id)?;
            let title = title.map(parse_title).transpose()?;
            let kind = kind.map(parse_kind).transpose()?;
            let loaded = repo.load_card(&id)?;
            let saved = repo.update_card(&id, &loaded.revision, |card| {
                if let Some(title) = title {
                    card.title = title;
                }
                if let Some(kind) = kind {
                    card.card_type = kind;
                }
            })?;
            println!("Updated: {}", saved.value.title);
        }
        CardCommand::Source { command } => change_source(repo, command)?,
    }
    Ok(())
}

fn modify_source<T: Eq>(values: &mut Vec<T>, source: T, add: bool) {
    if add {
        if !values.contains(&source) {
            values.push(source);
        }
    } else {
        values.retain(|value| value != &source);
    }
}

fn change_source(repo: &Repository, command: CardSourceCommand) -> Result<()> {
    enum SourceKind {
        Work,
        Asset,
        Card,
    }
    let (kind, action) = match command {
        CardSourceCommand::Work { command } => (SourceKind::Work, command),
        CardSourceCommand::Asset { command } => (SourceKind::Asset, command),
        CardSourceCommand::Card { command } => (SourceKind::Card, command),
    };
    let (add, id, source) = match action {
        SourceAction::Add { id, source } => (true, id, source),
        SourceAction::Remove { id, source } => (false, id, source),
    };
    let id = parse_id::<CardId>(&id)?;
    let loaded = repo.load_card(&id)?;
    match kind {
        SourceKind::Work => {
            let source = parse_id::<WorkId>(&source)?;
            if add {
                repo.load_work(&source)?;
            }
            if !add && !loaded.value.source_work_ids.contains(&source) {
                return Err(invalid("source Work is not referenced").into());
            }
            let saved = repo.update_card(&id, &loaded.revision, |card| {
                modify_source(&mut card.source_work_ids, source, add);
            })?;
            println!("Updated sources: {}", saved.value.title);
        }
        SourceKind::Asset => {
            let source = parse_id::<AssetId>(&source)?;
            if add {
                repo.load_asset(&source)?;
            }
            if !add && !loaded.value.source_asset_ids.contains(&source) {
                return Err(invalid("source Asset is not referenced").into());
            }
            let saved = repo.update_card(&id, &loaded.revision, |card| {
                modify_source(&mut card.source_asset_ids, source, add);
            })?;
            println!("Updated sources: {}", saved.value.title);
        }
        SourceKind::Card => {
            let source = parse_id::<CardId>(&source)?;
            if add {
                repo.load_card(&source)?;
            }
            if !add && !loaded.value.source_card_ids.contains(&source) {
                return Err(invalid("source Card is not referenced").into());
            }
            let saved = repo.update_card(&id, &loaded.revision, |card| {
                modify_source(&mut card.source_card_ids, source, add);
            })?;
            println!("Updated sources: {}", saved.value.title);
        }
    }
    Ok(())
}
