//! Work CLI: creation, discovery, editing and lifecycle management.
use super::editor;
use clap::Subcommand;
use digital_notes::{
    model::{
        SchemaVersion, SnakeCase, Title, UtcTimestamp, Work, WorkEntityType, WorkId, WorkStatus,
    },
    repository::Repository,
};
use std::{collections::HashMap, error::Error, fs, io};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Debug, Subcommand)]
pub enum WorkCommand {
    /// Create a Work before collecting its Assets.
    New {
        #[arg(long)]
        title: String,
        #[arg(long = "type", default_value = "source_collection")]
        kind: String,
        /// Read the initial Markdown body from a UTF-8 file.
        #[arg(long)]
        body_file: Option<std::path::PathBuf>,
        /// Add a reference to an existing Work (repeatable).
        #[arg(long = "source")]
        sources: Vec<String>,
        /// Print only the generated Work ID.
        #[arg(long)]
        id_only: bool,
    },
    /// List Works with titles first; IDs distinguish duplicate titles.
    List {
        #[arg(long)]
        json: bool,
        /// Include Work IDs even when titles are unique.
        #[arg(long)]
        ids: bool,
    },
    /// Display a Work by ID.
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Edit Markdown using $VISUAL/$EDITOR (or replace it from a UTF-8 file).
    Edit {
        id: String,
        #[arg(long)]
        body_file: Option<std::path::PathBuf>,
    },
    /// Update the title and/or Work type.
    Set {
        id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long = "type")]
        kind: Option<String>,
    },
    /// Mark a Work as done.
    Done { id: String },
    /// Reopen a completed Work.
    Reopen { id: String },
    /// Add or remove Work-to-Work source references.
    Source {
        #[command(subcommand)]
        command: WorkSourceCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum WorkSourceCommand {
    /// Reference an existing source Work.
    Add { id: String, source: String },
    /// Remove a source Work reference.
    Remove { id: String, source: String },
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn parse_id(value: &str) -> Result<WorkId> {
    Ok(value.parse::<WorkId>().map_err(invalid)?)
}

fn parse_title(value: String) -> Result<Title> {
    Ok(value.parse::<Title>().map_err(invalid)?)
}

fn parse_kind(value: String) -> Result<SnakeCase> {
    Ok(value.parse::<SnakeCase>().map_err(invalid)?)
}

fn status(status: WorkStatus) -> &'static str {
    match status {
        WorkStatus::Active => "active",
        WorkStatus::Done => "done",
    }
}

pub fn run(repo: &Repository, command: WorkCommand) -> Result<()> {
    match command {
        WorkCommand::New {
            title,
            kind,
            body_file,
            sources,
            id_only,
        } => {
            let title = parse_title(title)?;
            let work_type = parse_kind(kind)?;
            let body = match body_file {
                Some(path) => fs::read_to_string(path)?,
                None => String::new(),
            };
            let mut source_work_ids = Vec::with_capacity(sources.len());
            for value in sources {
                let id = parse_id(&value)?;
                // Repository validates self-reference and duplicates; the CLI
                // also checks the destination before creating a new Work.
                repo.load_work(&id)?;
                source_work_ids.push(id);
            }
            let timestamp = UtcTimestamp::now();
            let work = Work {
                schema_version: SchemaVersion,
                id: uuid::Uuid::new_v4().to_string().parse().map_err(invalid)?,
                entity_type: WorkEntityType::Work,
                title,
                created: timestamp,
                updated: timestamp,
                work_type,
                status: WorkStatus::Active,
                source_work_ids,
                body,
            };
            let saved = repo.create_work(work)?.value;
            if id_only {
                println!("{}", saved.id);
            } else {
                println!("Created: {}\nID: {}", saved.title, saved.id);
            }
        }
        WorkCommand::List { json, ids } => {
            let mut works = repo.list_works()?;
            works.sort_by(|a, b| a.title.as_str().cmp(b.title.as_str()).then(a.id.cmp(&b.id)));
            if json {
                println!("{}", serde_json::to_string_pretty(&works)?);
            } else {
                let mut title_counts = HashMap::<&str, usize>::new();
                for work in &works {
                    *title_counts.entry(work.title.as_str()).or_default() += 1;
                }
                for work in &works {
                    let show_id = ids || title_counts[work.title.as_str()] > 1;
                    if show_id {
                        println!(
                            "{} [{}]  ({} · {})",
                            work.title,
                            work.id,
                            work.work_type,
                            status(work.status)
                        );
                    } else {
                        println!(
                            "{}  ({} · {})",
                            work.title,
                            work.work_type,
                            status(work.status)
                        );
                    }
                }
            }
        }
        WorkCommand::Show { id, json } => {
            let loaded = repo.load_work(&parse_id(&id)?)?;
            let work = loaded.value;
            if json {
                println!("{}", serde_json::to_string_pretty(&work)?);
            } else {
                println!(
                    "{}\nID: {}\nType: {}\nStatus: {}\nCreated: {}\nUpdated: {}",
                    work.title,
                    work.id,
                    work.work_type,
                    status(work.status),
                    work.created,
                    work.updated
                );
                if !work.source_work_ids.is_empty() {
                    println!("Source Works:");
                    for id in &work.source_work_ids {
                        // Broken references should not make an otherwise valid
                        // Work impossible to inspect.
                        match repo.load_work(id) {
                            Ok(source) => println!("  - {} [{}]", source.value.title, id),
                            Err(_) => println!("  - [missing Work: {id}]"),
                        }
                    }
                }
                if !work.body.is_empty() {
                    println!("\n{}", work.body);
                }
            }
        }
        WorkCommand::Edit { id, body_file } => {
            let id = parse_id(&id)?;
            let loaded = repo.load_work(&id)?;
            if let Some(path) = body_file {
                let body = fs::read_to_string(path)?;
                let result = repo.update_work(&id, &loaded.revision, |work| work.body = body)?;
                println!("Saved: {}", result.value.title);
            } else {
                let edited = editor::edit_markdown(&loaded.value.body)?;
                let result = repo.update_work(&id, &loaded.revision, |work| {
                    work.body = edited.text.clone();
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
        WorkCommand::Set { id, title, kind } => {
            if title.is_none() && kind.is_none() {
                return Err(invalid("specify --title and/or --type").into());
            }
            let id = parse_id(&id)?;
            let title = title.map(parse_title).transpose()?;
            let kind = kind.map(parse_kind).transpose()?;
            let loaded = repo.load_work(&id)?;
            let saved = repo.update_work(&id, &loaded.revision, |work| {
                if let Some(title) = title {
                    work.title = title;
                }
                if let Some(kind) = kind {
                    work.work_type = kind;
                }
            })?;
            println!("Updated: {}", saved.value.title);
        }
        WorkCommand::Done { id } => change_status(repo, &id, WorkStatus::Done)?,
        WorkCommand::Reopen { id } => change_status(repo, &id, WorkStatus::Active)?,
        WorkCommand::Source { command } => change_source(repo, command)?,
    }
    Ok(())
}

fn change_status(repo: &Repository, raw_id: &str, new_status: WorkStatus) -> Result<()> {
    let id = parse_id(raw_id)?;
    let loaded = repo.load_work(&id)?;
    let saved = repo.update_work(&id, &loaded.revision, |work| work.status = new_status)?;
    println!("{}: {}", status(saved.value.status), saved.value.title);
    Ok(())
}

fn change_source(repo: &Repository, command: WorkSourceCommand) -> Result<()> {
    let (add, raw_id, raw_source) = match command {
        WorkSourceCommand::Add { id, source } => (true, id, source),
        WorkSourceCommand::Remove { id, source } => (false, id, source),
    };
    let id = parse_id(&raw_id)?;
    let source = parse_id(&raw_source)?;
    if add {
        repo.load_work(&source)?;
    }
    let loaded = repo.load_work(&id)?;
    if !add && !loaded.value.source_work_ids.contains(&source) {
        return Err(invalid("source Work is not referenced").into());
    }
    let saved = repo.update_work(&id, &loaded.revision, |work| {
        if add {
            if !work.source_work_ids.contains(&source) {
                work.source_work_ids.push(source);
            }
        } else {
            work.source_work_ids.retain(|value| value != &source);
        }
    })?;
    println!("Updated sources: {}", saved.value.title);
    Ok(())
}
