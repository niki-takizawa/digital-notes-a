//! Atomically decide whether to reuse or publish a completed SingleFile capture.
//!
//! The external capture runs *before* acquiring the repository lock. Matching,
//! Work linking, and publication happen while holding that lock to avoid two
//! identical captures becoming separate Assets accidentally.

use super::{
    Loaded, Repository, RepositoryError, Result,
    asset::{encode_asset, validate_asset},
};
use crate::model::{
    Asset, AssetEntityType, AssetId, ContentHash, HashAlgorithm, HtmlFileFormat, HttpUrl,
    SchemaVersion, SnakeCase, Title, UtcTimestamp, WorkId,
};
use sha2::{Digest, Sha256};
use std::fs;

#[derive(Debug)]
pub struct CaptureRequest {
    pub url: HttpUrl,
    pub title: Title,
    pub asset_type: SnakeCase,
    pub work_id: WorkId,
    /// Create a distinct Asset even when URL and HTML already match.
    pub force_new: bool,
    /// Resolve an ambiguous match or explicitly select an existing Asset.
    pub reuse_id: Option<AssetId>,
}

#[derive(Debug)]
pub enum CaptureOutcome {
    Created(Loaded<Asset>),
    Reused { asset: Loaded<Asset>, linked: bool },
}

impl CaptureOutcome {
    pub fn asset(&self) -> &Asset {
        match self {
            Self::Created(loaded) => &loaded.value,
            Self::Reused { asset, .. } => &asset.value,
        }
    }
}

impl Repository {
    /// List registered Assets, verifying each sidecar and its immutable HTML.
    /// Corrupt entries are errors, never silently ignored during deduplication.
    pub fn list_assets(&self) -> Result<Vec<Asset>> {
        let directory = self.root().join("assets");
        let entries = fs::read_dir(&directory).map_err(|error| super::io_at(&directory, error))?;
        let mut assets = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| super::io_at(&directory, error))?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let Some(id) = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| stem.parse::<AssetId>().ok())
            else {
                continue;
            };
            assets.push(self.load_asset(&id)?.value);
        }
        assets.sort_by(|a, b| a.title.as_str().cmp(b.title.as_str()).then(a.id.cmp(&b.id)));
        Ok(assets)
    }

    /// Register an already verified HTML snapshot; reuse only when both URL
    /// and exact HTML SHA-256 agree. Title/type of an existing Asset stay intact.
    /// All matching and writing is serialized under the repository write lock.
    pub fn publish_capture(&self, request: CaptureRequest, html: &[u8]) -> Result<CaptureOutcome> {
        if html.is_empty() {
            return Err(RepositoryError::InvalidAsset("empty HTML snapshot".into()));
        }
        if request.force_new && request.reuse_id.is_some() {
            return Err(RepositoryError::InvalidAsset(
                "--force-new cannot be combined with --reuse".into(),
            ));
        }
        let hash = format!("{:x}", Sha256::digest(html));
        let guard = self.begin_write()?;
        // A Work must exist both before SingleFile runs and when its Asset is saved.
        self.load_work(&request.work_id)?;

        let matches: Vec<_> = self
            .list_assets()?
            .into_iter()
            .filter(|asset| {
                asset.url == request.url && asset.content_hash.as_str() == hash.as_str()
            })
            .collect();

        let selected = if request.force_new {
            None
        } else if let Some(selected_id) = &request.reuse_id {
            Some(
                matches
                    .iter()
                    .find(|asset| &asset.id == selected_id)
                    .ok_or_else(|| {
                        RepositoryError::InvalidAsset("--reuse is not a matching Asset".into())
                    })?,
            )
        } else {
            match matches.len() {
                0 => None,
                1 => matches.first(),
                _ => {
                    return Err(RepositoryError::InvalidAsset(
                        "multiple matching Assets: choose one with --reuse or use --force-new"
                            .into(),
                    ));
                }
            }
        };

        if let Some(asset) = selected {
            let current = self.load_asset(&asset.id)?;
            if current.value.created_by_work_id == request.work_id
                || current.value.related_work_ids.contains(&request.work_id)
            {
                return Ok(CaptureOutcome::Reused {
                    asset: current,
                    linked: false,
                });
            }
            let mut updated = current.value.clone();
            updated.related_work_ids.push(request.work_id);
            updated.updated = UtcTimestamp::now().max(updated.updated);
            validate_asset(&updated)?;
            let path = self.asset_path(&updated.id);
            let bytes = encode_asset(&path, &updated)?;
            let revision = guard.replace_if_unchanged(&path, &current.revision, &bytes)?;
            return Ok(CaptureOutcome::Reused {
                asset: Loaded {
                    value: updated,
                    revision,
                },
                linked: true,
            });
        }

        let now = UtcTimestamp::now();
        let asset = Asset {
            schema_version: SchemaVersion,
            id: uuid::Uuid::new_v4()
                .to_string()
                .parse()
                .expect("UUID v4 is a valid Asset ID"),
            entity_type: AssetEntityType::Asset,
            title: request.title,
            created: now,
            updated: now,
            asset_type: request.asset_type,
            url: request.url,
            file_format: HtmlFileFormat::Html,
            capture_method: "singlefile".parse().expect("static snake_case"),
            created_by_work_id: request.work_id,
            related_work_ids: Vec::new(),
            content_hash: hash
                .parse::<ContentHash>()
                .expect("SHA-256 is lowercase hexadecimal"),
            hash_algorithm: HashAlgorithm::Sha256,
        };
        let loaded = self.create_asset_pair_locked(&guard, asset, html)?;
        Ok(CaptureOutcome::Created(loaded))
    }
}
