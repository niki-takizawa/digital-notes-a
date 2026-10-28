//! Asset repository operations: immutable HTML snapshots and mutable JSON metadata.
//!
//! An Asset becomes visible as a complete pair only after publishing its JSON
//! sidecar. The HTML is published first, so a process crash between the two
//! writes can leave an orphan HTML file. Commit 18's integrity scan will detect
//! such leftovers. This is not a two-file atomic transaction.

use super::{Loaded, Repository, RepositoryError, Result, Revision, WriteGuard};
use crate::model::{Asset, AssetId, UtcTimestamp};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, fs, io::ErrorKind, path::Path};

pub(super) fn validate_asset(asset: &Asset) -> Result<()> {
    if asset.updated < asset.created {
        return Err(RepositoryError::InvalidAsset(
            "updated must not precede created".to_owned(),
        ));
    }
    let mut seen = HashSet::new();
    for id in &asset.related_work_ids {
        if id == &asset.created_by_work_id {
            return Err(RepositoryError::InvalidAsset(
                "created_by_work_id must not also appear in related_work_ids".to_owned(),
            ));
        }
        if !seen.insert(id) {
            return Err(RepositoryError::InvalidAsset(format!(
                "duplicate related_work_id: {id}"
            )));
        }
    }
    Ok(())
}

fn verify_html(asset: &Asset, html: &[u8]) -> Result<()> {
    if html.is_empty() {
        return Err(RepositoryError::InvalidAsset(
            "HTML snapshot must not be empty".to_owned(),
        ));
    }
    let hash = Sha256::digest(html);
    let actual = format!("{hash:x}");
    if asset.content_hash.as_str() != actual {
        return Err(RepositoryError::InvalidAsset(format!(
            "HTML SHA-256 mismatch for Asset {}",
            asset.id
        )));
    }
    Ok(())
}

pub(super) fn encode_asset(path: &Path, asset: &Asset) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(asset).map_err(|source| RepositoryError::Json {
        path: path.to_owned(),
        source,
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

// Under the repository write lock, check both file names before publishing
// either. An orphan HTML file may indicate an interrupted previous creation;
// never overwrite it or silently treat it as our own.
fn ensure_absent(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(RepositoryError::AlreadyExists(path.to_owned())),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(super::io_at(path, err)),
    }
}

impl Repository {
    /// Verify all Work references while holding the write lock. Asset creation
    /// requires a pre-existing creator Work, even though CLI capture is not yet
    /// implemented. This also validates any additional Work relationships.
    fn verify_asset_works(&self, asset: &Asset) -> Result<()> {
        self.load_work(&asset.created_by_work_id)?;
        for id in &asset.related_work_ids {
            self.load_work(id)?;
        }
        Ok(())
    }

    /// Create `<id>.html` and `<id>.json` in `assets/`, without overwrites.
    /// HTML is published first; JSON is the final visibility marker.
    /// On interruption, an orphan HTML may remain and must be diagnosed.
    pub fn create_asset_pair(&self, asset: Asset, html: &[u8]) -> Result<Loaded<Asset>> {
        let guard = self.begin_write()?;
        self.create_asset_pair_locked(&guard, asset, html)
    }

    /// Shared with capture publication: the caller already owns the write lock.
    pub(super) fn create_asset_pair_locked(
        &self,
        guard: &WriteGuard<'_>,
        asset: Asset,
        html: &[u8],
    ) -> Result<Loaded<Asset>> {
        validate_asset(&asset)?;
        if asset.created != asset.updated {
            return Err(RepositoryError::InvalidAsset(
                "created and updated must match for a new Asset".to_owned(),
            ));
        }
        verify_html(&asset, html)?;
        self.verify_asset_works(&asset)?;
        let html_path = self.html_path(&asset.id);
        let json_path = self.asset_path(&asset.id);
        ensure_absent(&html_path)?;
        ensure_absent(&json_path)?;
        let bytes = encode_asset(&json_path, &asset)?;
        guard.create(&html_path, html)?;
        // If this fails (e.g., process crash, disk full), leave the HTML
        // untouched; it is immutable evidence and can be diagnosed later.
        let revision = guard.create(&json_path, &bytes)?;
        Ok(Loaded {
            value: asset,
            revision,
        })
    }

    /// Load and validate both parts of an Asset, including the SHA-256 of HTML.
    pub fn load_asset(&self, id: &AssetId) -> Result<Loaded<Asset>> {
        let json_path = self.asset_path(id);
        let loaded = self.read(&json_path)?;
        let asset: Asset =
            serde_json::from_slice(&loaded.bytes).map_err(|source| RepositoryError::Json {
                path: json_path.clone(),
                source,
            })?;
        if &asset.id != id {
            return Err(RepositoryError::InvalidAsset(format!(
                "Asset ID {} does not match file name {id}",
                asset.id
            )));
        }
        validate_asset(&asset)?;
        let html = self.read(&self.html_path(id))?;
        verify_html(&asset, &html.bytes)?;
        Ok(Loaded {
            value: asset,
            revision: loaded.revision,
        })
    }

    /// Load an Asset's HTML bytes only if the sidecar and SHA-256 agree.
    pub fn load_asset_html(&self, id: &AssetId) -> Result<Vec<u8>> {
        let asset = self.load_asset(id)?;
        let bytes = self.read(&self.html_path(id))?.bytes;
        verify_html(&asset.value, &bytes)?;
        Ok(bytes)
    }

    /// Mutate metadata using its last-seen JSON revision. Asset provenance,
    /// capture URL and original HTML hash are immutable. A no-op does not write.
    pub fn update_asset(
        &self,
        id: &AssetId,
        expected: &Revision,
        mutate: impl FnOnce(&mut Asset),
    ) -> Result<Loaded<Asset>> {
        let guard = self.begin_write()?;
        let current = self.load_asset(id)?;
        let json_path = self.asset_path(id);
        if &current.revision != expected {
            return Err(RepositoryError::Conflict(json_path));
        }
        let mut proposed = current.value.clone();
        mutate(&mut proposed);
        if proposed.schema_version != current.value.schema_version
            || proposed.id != current.value.id
            || proposed.entity_type != current.value.entity_type
            || proposed.created != current.value.created
            || proposed.updated != current.value.updated
            || proposed.created_by_work_id != current.value.created_by_work_id
            || proposed.url != current.value.url
            || proposed.file_format != current.value.file_format
            || proposed.capture_method != current.value.capture_method
            || proposed.content_hash != current.value.content_hash
            || proposed.hash_algorithm != current.value.hash_algorithm
        {
            return Err(RepositoryError::InvalidAsset(
                "Asset identity, provenance, capture fields and updated are immutable".to_owned(),
            ));
        }
        validate_asset(&proposed)?;
        self.verify_asset_works(&proposed)?;
        if proposed == current.value {
            return Ok(current);
        }
        proposed.updated = UtcTimestamp::now().max(current.value.updated);
        let bytes = encode_asset(&json_path, &proposed)?;
        let revision = guard.replace_if_unchanged(&json_path, expected, &bytes)?;
        Ok(Loaded {
            value: proposed,
            revision,
        })
    }
}
