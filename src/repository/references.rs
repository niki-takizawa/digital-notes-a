//! Cross-entity reference and Asset-pair integrity audit.
//!
//! This is an explicit repository-wide audit, not a mutation-time gate.
//! It reuses the typed loaders, so malformed JSON, local invariants and
//! Asset HTML hashes are checked without duplicating their logic.

use super::{Repository, Result, io_at};
use crate::model::{Asset, AssetId, Card, CardId, Work, WorkId};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum IntegrityIssueKind {
    MissingWork,
    MissingAsset,
    MissingCard,
    OrphanAssetHtml,
    MissingAssetHtml,
    InvalidEntry,
    InvalidEntity,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct IntegrityIssue {
    /// The file containing the broken reference, or the problematic entry.
    pub path: PathBuf,
    pub kind: IntegrityIssueKind,
    pub detail: String,
}

fn issue(
    issues: &mut Vec<IntegrityIssue>,
    path: &Path,
    kind: IntegrityIssueKind,
    detail: impl Into<String>,
) {
    issues.push(IntegrityIssue {
        path: path.to_owned(),
        kind,
        detail: detail.into(),
    });
}

/// Reject unexpected names, symlinks, and non-regular files without following
/// them. A failure to enumerate the directory is a fatal I/O error instead.
fn entries(dir: &Path, issues: &mut Vec<IntegrityIssue>) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(dir).map_err(|error| io_at(dir, error))? {
        let entry = entry.map_err(|error| io_at(dir, error))?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|error| io_at(&path, error))?;
        if kind.is_file() {
            paths.push(path);
        } else {
            issue(
                issues,
                &path,
                IntegrityIssueKind::InvalidEntry,
                "expected a regular file (symlinks are forbidden)",
            );
        }
    }
    paths.sort();
    Ok(paths)
}

fn file_id<T: std::str::FromStr>(path: &Path, extension: &str) -> Option<T> {
    if path.extension()?.to_str()? != extension {
        return None;
    }
    path.file_stem()?.to_str()?.parse().ok()
}

fn missing_work(
    issues: &mut Vec<IntegrityIssue>,
    origin: &Path,
    role: &str,
    id: &WorkId,
    available: &BTreeSet<WorkId>,
) {
    if !available.contains(id) {
        issue(
            issues,
            origin,
            IntegrityIssueKind::MissingWork,
            format!("{role}: {id}"),
        );
    }
}

fn missing_asset(
    issues: &mut Vec<IntegrityIssue>,
    origin: &Path,
    role: &str,
    id: &AssetId,
    available: &BTreeSet<AssetId>,
) {
    if !available.contains(id) {
        issue(
            issues,
            origin,
            IntegrityIssueKind::MissingAsset,
            format!("{role}: {id}"),
        );
    }
}

fn missing_card(
    issues: &mut Vec<IntegrityIssue>,
    origin: &Path,
    role: &str,
    id: &CardId,
    available: &BTreeSet<CardId>,
) {
    if !available.contains(id) {
        issue(
            issues,
            origin,
            IntegrityIssueKind::MissingCard,
            format!("{role}: {id}"),
        );
    }
}

impl Repository {
    /// Audit all persisted references and Asset file pairs without modifying
    /// Entity files. The write lock ensures a consistent view relative to
    /// repository writers (external edits must still be coordinated).
    ///
    /// Invalid entities are reported individually; references to their file
    /// names are not additionally reported as missing. A separate `check`
    /// command can consume these structured issues in a later commit.
    pub fn check_references(&self) -> Result<Vec<IntegrityIssue>> {
        let _guard = self.begin_write()?;
        let mut issues = Vec::new();
        let mut works: Vec<Work> = Vec::new();
        let mut assets: Vec<Asset> = Vec::new();
        let mut cards: Vec<Card> = Vec::new();
        let mut work_names = BTreeSet::new();
        let mut asset_names = BTreeSet::new();
        let mut card_names = BTreeSet::new();

        for path in entries(&self.root().join("works"), &mut issues)? {
            let Some(id) = file_id::<WorkId>(&path, "json") else {
                issue(
                    &mut issues,
                    &path,
                    IntegrityIssueKind::InvalidEntry,
                    "invalid Work file name",
                );
                continue;
            };
            work_names.insert(id.clone());
            match self.load_work(&id) {
                Ok(loaded) => works.push(loaded.value),
                Err(error) => issue(
                    &mut issues,
                    &path,
                    IntegrityIssueKind::InvalidEntity,
                    error.to_string(),
                ),
            }
        }

        // Classify Asset HTML and sidecar files before loading pairs.
        let mut pairs: BTreeMap<AssetId, (bool, bool)> = BTreeMap::new();
        for path in entries(&self.root().join("assets"), &mut issues)? {
            let Some(extension) = path.extension().and_then(|s| s.to_str()) else {
                issue(
                    &mut issues,
                    &path,
                    IntegrityIssueKind::InvalidEntry,
                    "invalid Asset file name",
                );
                continue;
            };
            if extension != "html" && extension != "json" {
                issue(
                    &mut issues,
                    &path,
                    IntegrityIssueKind::InvalidEntry,
                    "unexpected Asset file extension",
                );
                continue;
            }
            let Some(id) = file_id::<AssetId>(&path, extension) else {
                issue(
                    &mut issues,
                    &path,
                    IntegrityIssueKind::InvalidEntry,
                    "invalid Asset ID in file name",
                );
                continue;
            };
            let pair = pairs.entry(id).or_default();
            if extension == "json" {
                pair.0 = true;
            } else {
                pair.1 = true;
            }
        }
        for (id, (has_json, has_html)) in pairs {
            if has_html && !has_json {
                issue(
                    &mut issues,
                    &self.html_path(&id),
                    IntegrityIssueKind::OrphanAssetHtml,
                    format!("Asset {id} has no JSON sidecar"),
                );
            } else if has_json && !has_html {
                asset_names.insert(id.clone());
                issue(
                    &mut issues,
                    &self.asset_path(&id),
                    IntegrityIssueKind::MissingAssetHtml,
                    format!("Asset {id} has no HTML snapshot"),
                );
            } else {
                asset_names.insert(id.clone());
                match self.load_asset(&id) {
                    Ok(loaded) => assets.push(loaded.value),
                    Err(error) => issue(
                        &mut issues,
                        &self.asset_path(&id),
                        IntegrityIssueKind::InvalidEntity,
                        error.to_string(),
                    ),
                }
            }
        }

        for path in entries(&self.root().join("cards"), &mut issues)? {
            let Some(id) = file_id::<CardId>(&path, "json") else {
                issue(
                    &mut issues,
                    &path,
                    IntegrityIssueKind::InvalidEntry,
                    "invalid Card file name",
                );
                continue;
            };
            card_names.insert(id.clone());
            match self.load_card(&id) {
                Ok(loaded) => cards.push(loaded.value),
                Err(error) => issue(
                    &mut issues,
                    &path,
                    IntegrityIssueKind::InvalidEntity,
                    error.to_string(),
                ),
            }
        }

        for work in &works {
            let path = self.work_path(&work.id);
            for id in &work.source_work_ids {
                missing_work(&mut issues, &path, "source_work_id", id, &work_names);
            }
        }
        for asset in &assets {
            let path = self.asset_path(&asset.id);
            missing_work(
                &mut issues,
                &path,
                "created_by_work_id",
                &asset.created_by_work_id,
                &work_names,
            );
            for id in &asset.related_work_ids {
                missing_work(&mut issues, &path, "related_work_id", id, &work_names);
            }
        }
        for card in &cards {
            let path = self.card_path(&card.id);
            missing_work(
                &mut issues,
                &path,
                "created_by_work_id",
                &card.created_by_work_id,
                &work_names,
            );
            for id in &card.source_work_ids {
                missing_work(&mut issues, &path, "source_work_id", id, &work_names);
            }
            for id in &card.source_asset_ids {
                missing_asset(&mut issues, &path, "source_asset_id", id, &asset_names);
            }
            for id in &card.source_card_ids {
                missing_card(&mut issues, &path, "source_card_id", id, &card_names);
            }
            // Card-local validation ensures every Evidence Asset belongs to
            // source_asset_ids, so the check above covers Evidence references.
        }

        issues.sort();
        Ok(issues)
    }
}
