//! Evidence registration and verification for immutable Asset snapshots.
//!
//! The Asset HTML is validated before any Card mutation. A single
//! revision-checked Card update publishes the Evidence, source link and
//! Markdown citation marker together.

use super::{Loaded, Repository, RepositoryError, Result, Revision};
use crate::{
    evidence::{self, Resolution},
    model::{AssetId, Card, CardId, EvidenceId},
    text,
};

fn invalid(message: impl Into<String>) -> RepositoryError {
    RepositoryError::InvalidCard(message.into())
}

fn next_evidence_id(card: &Card) -> Result<EvidenceId> {
    let mut maximum = 0_u64;
    for entry in &card.evidence {
        let raw = entry.id.to_string();
        let digits = raw
            .strip_prefix("ev")
            .ok_or_else(|| invalid("invalid Evidence ID prefix"))?;
        let number = digits
            .parse::<u64>()
            .map_err(|_| invalid("Evidence ID exceeds u64"))?;
        maximum = maximum.max(number);
    }
    loop {
        maximum = maximum
            .checked_add(1)
            .ok_or_else(|| invalid("Evidence ID overflow"))?;
        let id: EvidenceId = format!("ev{maximum}").parse().map_err(invalid)?;
        // Do not overwrite a marker a user may have inserted independently.
        if !card.body.contains(&format!("[^{id}]")) {
            return Ok(id);
        }
    }
}

/// A Markdown marker uses the Evidence ID, not the Asset ID. An explicit
/// offset is counted in Unicode codepoints and must be a grapheme boundary.
fn insert_marker(body: &mut String, id: &EvidenceId, offset: Option<usize>) -> Result<()> {
    let marker = format!("[^{id}]");
    if body.contains(&marker) {
        return Err(invalid(format!(
            "marker {marker} already exists in Card body"
        )));
    }
    match offset {
        Some(position) => {
            let byte = text::cp_to_byte(body, position)
                .ok_or_else(|| invalid("marker offset exceeds Card body length"))?;
            if !text::grapheme_boundary(body, position) {
                return Err(invalid("marker offset divides a grapheme cluster"));
            }
            body.insert_str(byte, &marker);
        }
        None if body.is_empty() => body.push_str(&marker),
        None => {
            if !body.ends_with('\n') {
                body.push('\n');
            }
            body.push('\n');
            body.push_str(&marker);
        }
    }
    Ok(())
}

impl Repository {
    /// Read immutable HTML, check the sidecar hash, then normalize its text.
    pub fn asset_text(&self, id: &AssetId) -> Result<String> {
        let html = self.load_asset_html(id)?;
        text::html_text_v1(&html)
            .map_err(|error| RepositoryError::InvalidAsset(format!("Asset {id}: {error}")))
    }

    /// Add a verified citation to a Card. A stale Card revision is rejected.
    /// No Card file is changed if Asset, quote, marker or revision validation fails.
    pub fn add_card_evidence(
        &self,
        id: &CardId,
        expected: &Revision,
        asset_id: &AssetId,
        quote: &str,
        start: Option<usize>,
        marker_offset: Option<usize>,
    ) -> Result<Loaded<Card>> {
        let current = self.load_card(id)?;
        if &current.revision != expected {
            return Err(RepositoryError::Conflict(self.card_path(id)));
        }
        let text = self.asset_text(asset_id)?;
        let evidence = evidence::make_evidence(
            next_evidence_id(&current.value)?,
            asset_id.clone(),
            &text,
            quote,
            start,
        )
        .map_err(|error| invalid(error.to_string()))?;
        let mut body = current.value.body.clone();
        insert_marker(&mut body, &evidence.id, marker_offset)?;

        self.update_card(id, expected, move |card| {
            if !card.source_asset_ids.contains(asset_id) {
                card.source_asset_ids.push(asset_id.clone());
            }
            card.evidence.push(evidence);
            card.body = body;
        })
    }

    /// Check each saved quote against the current validated HTML snapshot.
    /// Does not rewrite selectors when their positions have moved.
    pub fn verify_card_evidence(&self, id: &CardId) -> Result<Vec<(EvidenceId, Resolution)>> {
        let card = self.load_card(id)?.value;
        card.evidence
            .iter()
            .map(|entry| {
                let text = self.asset_text(&entry.asset_id)?;
                Ok((entry.id.clone(), evidence::resolve(&text, entry)))
            })
            .collect()
    }
}
