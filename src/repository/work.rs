//! Work operations: typed JSON persistence and revision-checked mutations.
//!
//! Reference existence checks are deliberately deferred to the repository's
//! cross-entity validation layer. This module enforces local Work invariants.

use super::{Loaded, Repository, RepositoryError, Result, Revision};
use crate::model::{UtcTimestamp, Work, WorkId};
use std::{collections::HashSet, path::Path};

fn validate_work(work: &Work) -> Result<()> {
    if work.updated < work.created {
        return Err(RepositoryError::InvalidWork(
            "updated must not precede created".to_owned(),
        ));
    }
    let mut seen = HashSet::new();
    for reference in &work.source_work_ids {
        if reference == &work.id {
            return Err(RepositoryError::InvalidWork(
                "a Work cannot reference itself".to_owned(),
            ));
        }
        if !seen.insert(reference) {
            return Err(RepositoryError::InvalidWork(format!(
                "duplicate source_work_id: {reference}"
            )));
        }
    }
    Ok(())
}

fn encode_work(path: &Path, work: &Work) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(work).map_err(|source| RepositoryError::Json {
        path: path.to_owned(),
        source,
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

impl Repository {
    /// Create a Work before the Assets or Cards it will be associated with.
    /// On creation `created` and `updated` must be equal.
    pub fn create_work(&self, work: Work) -> Result<Loaded<Work>> {
        let guard = self.begin_write()?;
        validate_work(&work)?;
        if work.updated != work.created {
            return Err(RepositoryError::InvalidWork(
                "created and updated must match for a new Work".to_owned(),
            ));
        }
        let path = self.work_path(&work.id);
        let bytes = encode_work(&path, &work)?;
        let revision = guard.create(&path, &bytes)?;
        Ok(Loaded {
            value: work,
            revision,
        })
    }

    /// Load a Work and reject malformed content or a filename/ID mismatch.
    pub fn load_work(&self, id: &WorkId) -> Result<Loaded<Work>> {
        let path = self.work_path(id);
        let loaded = self.read(&path)?;
        let work: Work =
            serde_json::from_slice(&loaded.bytes).map_err(|source| RepositoryError::Json {
                path: path.clone(),
                source,
            })?;
        if &work.id != id {
            return Err(RepositoryError::InvalidWork(format!(
                "Work ID {} does not match file name {id}",
                work.id
            )));
        }
        validate_work(&work)?;
        Ok(Loaded {
            value: work,
            revision: loaded.revision,
        })
    }

    /// Update mutable Work fields using compare-and-swap semantics.
    ///
    /// `id`, `schema_version`, `entity_type`, `created`, and `updated` are
    /// immutable to callers; `updated` is managed here. A no-op does not write
    /// a file or change its revision. The closure runs while holding the lock.
    pub fn update_work(
        &self,
        id: &WorkId,
        expected: &Revision,
        mutate: impl FnOnce(&mut Work),
    ) -> Result<Loaded<Work>> {
        let guard = self.begin_write()?;
        let current = self.load_work(id)?;
        let path = self.work_path(id);
        if &current.revision != expected {
            return Err(RepositoryError::Conflict(path));
        }

        let mut proposed = current.value.clone();
        mutate(&mut proposed);
        if proposed.id != current.value.id
            || proposed.schema_version != current.value.schema_version
            || proposed.entity_type != current.value.entity_type
            || proposed.created != current.value.created
            || proposed.updated != current.value.updated
        {
            return Err(RepositoryError::InvalidWork(
                "id, schema_version, entity_type, created and updated are immutable".to_owned(),
            ));
        }
        validate_work(&proposed)?;
        if proposed == current.value {
            return Ok(current);
        }

        proposed.updated = UtcTimestamp::now().max(current.value.updated);
        let bytes = encode_work(&path, &proposed)?;
        let revision = guard.replace_if_unchanged(&path, expected, &bytes)?;
        Ok(Loaded {
            value: proposed,
            revision,
        })
    }
}

impl Repository {
    /// Enumerate valid Work filenames, checking each Work during loading.
    /// Unrelated files are ignored; corrupt Work JSON is reported as an error.
    pub fn list_works(&self) -> Result<Vec<Work>> {
        let directory = self.root().join("works");
        let entries =
            std::fs::read_dir(&directory).map_err(|error| super::io_at(&directory, error))?;
        let mut works = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| super::io_at(&directory, error))?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let Some(id) = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| stem.parse::<WorkId>().ok())
            else {
                continue;
            };
            works.push(self.load_work(&id)?.value);
        }
        works.sort_by(|a, b| a.title.as_str().cmp(b.title.as_str()).then(a.id.cmp(&b.id)));
        Ok(works)
    }
}
