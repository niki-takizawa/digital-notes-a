//! Card persistence: creator Work, multi-source references and Evidence metadata.
//!
//! This layer checks Card-local invariants and that its creator Work exists.
//! Other referenced Entity IDs are checked by the cross-entity layer in Commit 18;
//! verifying quote/position selectors against HTML belongs to Commit 24-25.

use super::{Loaded, Repository, RepositoryError, Result, Revision};
use crate::model::{Card, CardId, UtcTimestamp};
use std::{collections::HashSet, path::Path};

fn validate_card(card: &Card) -> Result<()> {
    if card.updated < card.created {
        return Err(RepositoryError::InvalidCard(
            "updated must not precede created".to_owned(),
        ));
    }

    let mut work_ids = HashSet::new();
    for id in &card.source_work_ids {
        if id == &card.created_by_work_id {
            return Err(RepositoryError::InvalidCard(
                "created_by_work_id must not appear in source_work_ids".to_owned(),
            ));
        }
        if !work_ids.insert(id) {
            return Err(RepositoryError::InvalidCard(format!(
                "duplicate source_work_id: {id}"
            )));
        }
    }

    let mut asset_ids = HashSet::new();
    for id in &card.source_asset_ids {
        if !asset_ids.insert(id.clone()) {
            return Err(RepositoryError::InvalidCard(format!(
                "duplicate source_asset_id: {id}"
            )));
        }
    }

    let mut card_ids = HashSet::new();
    for id in &card.source_card_ids {
        if id == &card.id {
            return Err(RepositoryError::InvalidCard(
                "a Card cannot reference itself".to_owned(),
            ));
        }
        if !card_ids.insert(id) {
            return Err(RepositoryError::InvalidCard(format!(
                "duplicate source_card_id: {id}"
            )));
        }
    }

    let mut evidence_ids = HashSet::new();
    for evidence in &card.evidence {
        if !evidence_ids.insert(&evidence.id) {
            return Err(RepositoryError::InvalidCard(format!(
                "duplicate evidence ID: {}",
                evidence.id
            )));
        }
        if !asset_ids.contains(&evidence.asset_id) {
            return Err(RepositoryError::InvalidCard(format!(
                "evidence {} refers to an Asset missing from source_asset_ids",
                evidence.id
            )));
        }
        if evidence.selector.exact.is_empty() {
            return Err(RepositoryError::InvalidCard(format!(
                "evidence {} has an empty exact quote",
                evidence.id
            )));
        }
        if evidence.position_selector.start >= evidence.position_selector.end {
            return Err(RepositoryError::InvalidCard(format!(
                "evidence {} has a non-increasing text range",
                evidence.id
            )));
        }
    }
    Ok(())
}

fn encode_card(path: &Path, card: &Card) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(card).map_err(|source| RepositoryError::Json {
        path: path.to_owned(),
        source,
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

impl Repository {
    /// Enumerate Cards by valid filenames, validating each on load.
    /// Unexpected extensions are ignored; invalid JSON is an error.
    pub fn list_cards(&self) -> Result<Vec<Card>> {
        let directory = self.root().join("cards");
        let entries =
            std::fs::read_dir(&directory).map_err(|error| super::io_at(&directory, error))?;
        let mut cards = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| super::io_at(&directory, error))?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let Some(id) = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| stem.parse::<CardId>().ok())
            else {
                continue;
            };
            cards.push(self.load_card(&id)?.value);
        }
        cards.sort_by(|a, b| a.title.as_str().cmp(b.title.as_str()).then(a.id.cmp(&b.id)));
        Ok(cards)
    }
    /// Create a Card within a pre-existing Work. A newly created Card has
    /// identical `created` and `updated` timestamps.
    pub fn create_card(&self, card: Card) -> Result<Loaded<Card>> {
        let guard = self.begin_write()?;
        validate_card(&card)?;
        if card.created != card.updated {
            return Err(RepositoryError::InvalidCard(
                "created and updated must match for a new Card".to_owned(),
            ));
        }
        self.load_work(&card.created_by_work_id)?;
        let path = self.card_path(&card.id);
        let bytes = encode_card(&path, &card)?;
        let revision = guard.create(&path, &bytes)?;
        Ok(Loaded {
            value: card,
            revision,
        })
    }

    /// Read a Card, verifying its filename, JSON structure and local invariants.
    pub fn load_card(&self, id: &CardId) -> Result<Loaded<Card>> {
        let path = self.card_path(id);
        let loaded = self.read(&path)?;
        let card: Card =
            serde_json::from_slice(&loaded.bytes).map_err(|source| RepositoryError::Json {
                path: path.clone(),
                source,
            })?;
        if &card.id != id {
            return Err(RepositoryError::InvalidCard(format!(
                "Card ID {} does not match file name {id}",
                card.id
            )));
        }
        validate_card(&card)?;
        Ok(Loaded {
            value: card,
            revision: loaded.revision,
        })
    }

    /// Mutate only editable Card fields, with an exact-revision compare-and-swap.
    /// A no-op does not rewrite the file. The mutation runs under the write lock.
    pub fn update_card(
        &self,
        id: &CardId,
        expected: &Revision,
        mutate: impl FnOnce(&mut Card),
    ) -> Result<Loaded<Card>> {
        let guard = self.begin_write()?;
        let current = self.load_card(id)?;
        let path = self.card_path(id);
        if &current.revision != expected {
            return Err(RepositoryError::Conflict(path));
        }

        let mut proposed = current.value.clone();
        mutate(&mut proposed);
        if proposed.schema_version != current.value.schema_version
            || proposed.id != current.value.id
            || proposed.entity_type != current.value.entity_type
            || proposed.created != current.value.created
            || proposed.updated != current.value.updated
            || proposed.created_by_work_id != current.value.created_by_work_id
        {
            return Err(RepositoryError::InvalidCard(
                "Card identity, creation metadata and updated are immutable".to_owned(),
            ));
        }
        validate_card(&proposed)?;
        self.load_work(&proposed.created_by_work_id)?;
        if proposed == current.value {
            return Ok(current);
        }

        proposed.updated = UtcTimestamp::now().max(current.value.updated);
        let bytes = encode_card(&path, &proposed)?;
        let revision = guard.replace_if_unchanged(&path, expected, &bytes)?;
        Ok(Loaded {
            value: proposed,
            revision,
        })
    }
}
