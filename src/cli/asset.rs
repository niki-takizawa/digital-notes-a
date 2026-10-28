//! CLI commands for collecting SingleFile snapshots and browsing saved Assets.

use clap::Subcommand;
use digital_notes::{
    capture::SingleFileCapture,
    model::{AssetId, HttpUrl, SnakeCase, Title, WorkId},
    repository::{CaptureOutcome, CaptureRequest, Repository},
};
use std::{collections::HashMap, error::Error, io, path::PathBuf};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Debug, Subcommand)]
pub enum AssetCommand {
    /// Capture a page and register its immutable HTML in an existing Work.
    Capture {
        /// An http(s) URL, passed directly to SingleFile.
        url: String,
        /// The collecting Work must already exist.
        #[arg(long)]
        work: String,
        #[arg(long = "type", default_value = "web_page")]
        kind: String,
        /// If omitted, the URL is used as the display title.
        #[arg(long)]
        title: Option<String>,
        /// Create a new Asset even if URL and snapshot bytes match.
        #[arg(long, conflicts_with = "reuse")]
        force_new: bool,
        /// Explicitly select one matching Asset to reuse.
        #[arg(long, value_name = "ASSET_ID")]
        reuse: Option<String>,
        #[arg(long)]
        id_only: bool,
        /// Override the SingleFile executable; also configurable by environment.
        #[arg(
            long,
            env = "DIGITAL_NOTES_SINGLE_FILE",
            default_value = "single-file",
            value_name = "PATH"
        )]
        single_file: PathBuf,
    },
    /// List registered Assets by title.
    List {
        #[arg(long)]
        json: bool,
        #[arg(long)]
        ids: bool,
    },
    /// Show a saved Asset's metadata.
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn parse<T: std::str::FromStr<Err = String>>(raw: &str) -> Result<T> {
    Ok(raw.parse::<T>().map_err(invalid)?)
}

pub fn run(repo: &Repository, command: AssetCommand) -> Result<()> {
    match command {
        AssetCommand::Capture {
            url,
            work,
            kind,
            title,
            force_new,
            reuse,
            id_only,
            single_file,
        } => {
            let url: HttpUrl = parse(&url)?;
            let work_id: WorkId = parse(&work)?;
            let asset_type: SnakeCase = parse(&kind)?;
            let title: Title = parse(title.as_deref().unwrap_or(url.as_str()))?;
            let reuse_id: Option<AssetId> = reuse.as_deref().map(parse).transpose()?;
            // Fail before launching the browser if the collecting Work is missing.
            repo.load_work(&work_id)?;
            let html = SingleFileCapture::new(single_file.into_os_string()).capture(&url)?;
            let outcome = repo.publish_capture(
                CaptureRequest {
                    url,
                    title,
                    asset_type,
                    work_id,
                    force_new,
                    reuse_id,
                },
                &html,
            )?;
            if id_only {
                println!("{}", outcome.asset().id);
            } else {
                match outcome {
                    CaptureOutcome::Created(saved) => {
                        println!("Created: {}\nID: {}", saved.value.title, saved.value.id);
                    }
                    CaptureOutcome::Reused { asset, linked } => {
                        let note = if linked { " (linked to Work)" } else { "" };
                        println!(
                            "Reused: {}\nID: {}{note}",
                            asset.value.title, asset.value.id
                        );
                    }
                }
            }
        }
        AssetCommand::List { json, ids } => {
            let assets = repo.list_assets()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&assets)?);
            } else {
                let mut counts = HashMap::<&str, usize>::new();
                for asset in &assets {
                    *counts.entry(asset.title.as_str()).or_default() += 1;
                }
                for asset in &assets {
                    if ids || counts[asset.title.as_str()] > 1 {
                        println!("{} [{}]  ({})", asset.title, asset.id, asset.asset_type);
                    } else {
                        println!("{}  ({})", asset.title, asset.asset_type);
                    }
                }
            }
        }
        AssetCommand::Show { id, json } => {
            let asset = repo.load_asset(&parse::<AssetId>(&id)?)?.value;
            if json {
                println!("{}", serde_json::to_string_pretty(&asset)?);
            } else {
                println!(
                    "{}\nID: {}\nType: {}\nURL: {}\nCreated: {}\nUpdated: {}",
                    asset.title,
                    asset.id,
                    asset.asset_type,
                    asset.url,
                    asset.created,
                    asset.updated
                );
                let creator = repo
                    .load_work(&asset.created_by_work_id)
                    .map(|work| work.value.title.to_string())
                    .unwrap_or_else(|_| "[missing Work]".to_owned());
                println!("Created by: {creator} [{}]", asset.created_by_work_id);
                if !asset.related_work_ids.is_empty() {
                    println!("Related Works:");
                    for id in &asset.related_work_ids {
                        let title = repo
                            .load_work(id)
                            .map(|work| work.value.title.to_string())
                            .unwrap_or_else(|_| "[missing Work]".to_owned());
                        println!("  - {title} [{id}]");
                    }
                }
                println!("SHA-256: {}", asset.content_hash);
            }
        }
    }
    Ok(())
}
