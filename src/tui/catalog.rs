//! Read-only snapshots for the first TUI viewer.
//! Every displayed value is escaped for safe terminal rendering.

use digital_notes::{
    model::{Asset, Card, Work, WorkStatus},
    repository::{Repository, Result},
};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Kind {
    Work,
    Asset,
    Card,
}

impl Kind {
    pub(super) fn index(self) -> usize {
        match self {
            Self::Work => 0,
            Self::Asset => 1,
            Self::Card => 2,
        }
    }

    pub(super) fn from_index(index: usize) -> Self {
        match index % 3 {
            0 => Self::Work,
            1 => Self::Asset,
            _ => Self::Card,
        }
    }

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Work => "Works",
            Self::Asset => "Assets",
            Self::Card => "Cards",
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct Entry {
    pub(super) title: String,
    pub(super) id: String,
    pub(super) label: String,
    pub(super) detail: String,
}

impl Entry {
    fn new(title: &str, id: String, detail: String) -> Self {
        let title = terminal_safe(title, false);
        Self {
            label: title.clone(),
            title,
            id,
            detail,
        }
    }
}

/// Prevent control sequences from entering Ratatui output when data is untrusted.
fn terminal_safe(value: &str, multiline: bool) -> String {
    value
        .chars()
        .filter_map(|c| match c {
            '\n' if multiline => Some('\n'),
            '\r' | '\n' | '\t' => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect()
}

fn disambiguate(entries: &mut [Entry]) {
    let mut counts = HashMap::<String, usize>::new();
    for entry in entries.iter() {
        *counts.entry(entry.title.clone()).or_default() += 1;
    }
    for entry in entries.iter_mut() {
        if counts.get(entry.title.as_str()).copied().unwrap_or(0) > 1 {
            entry.label = format!("{} [{}]", entry.title, entry.id);
        }
    }
}

fn from_work(work: Work) -> Entry {
    let status = match work.status {
        WorkStatus::Active => "active",
        WorkStatus::Done => "done",
    };
    let id = work.id.to_string();
    let detail = format!(
        "{}\n\nID: {}\nType: {}\nStatus: {}\nCreated: {}\nUpdated: {}\nSource Works: {}\n\nMarkdown body:\n{}",
        work.title,
        id,
        work.work_type,
        status,
        work.created,
        work.updated,
        if work.source_work_ids.is_empty() {
            "(none)".to_owned()
        } else {
            work.source_work_ids
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        },
        work.body,
    );
    Entry::new(work.title.as_str(), id, terminal_safe(&detail, true))
}

fn from_asset(asset: Asset) -> Entry {
    let id = asset.id.to_string();
    let related = if asset.related_work_ids.is_empty() {
        "(none)".to_owned()
    } else {
        asset
            .related_work_ids
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let detail = format!(
        "{}\n\nID: {}\nType: {}\nURL: {}\nCreated: {}\nUpdated: {}\nCreated by Work: {}\nRelated Works: {}\nSHA-256: {}",
        asset.title,
        id,
        asset.asset_type,
        asset.url,
        asset.created,
        asset.updated,
        asset.created_by_work_id,
        related,
        asset.content_hash
    );
    Entry::new(asset.title.as_str(), id, terminal_safe(&detail, true))
}

fn from_card(card: Card) -> Entry {
    let id = card.id.to_string();
    let sources = |ids: Vec<String>| {
        if ids.is_empty() {
            "(none)".to_owned()
        } else {
            ids.join(", ")
        }
    };
    let detail = format!(
        "{}\n\nID: {}\nType: {}\nCreated: {}\nUpdated: {}\nCreated by Work: {}\nSource Works: {}\nSource Assets: {}\nSource Cards: {}\nEvidence entries: {}\n\nMarkdown body:\n{}",
        card.title,
        id,
        card.card_type,
        card.created,
        card.updated,
        card.created_by_work_id,
        sources(
            card.source_work_ids
                .iter()
                .map(ToString::to_string)
                .collect()
        ),
        sources(
            card.source_asset_ids
                .iter()
                .map(ToString::to_string)
                .collect()
        ),
        sources(
            card.source_card_ids
                .iter()
                .map(ToString::to_string)
                .collect()
        ),
        card.evidence.len(),
        card.body,
    );
    Entry::new(card.title.as_str(), id, terminal_safe(&detail, true))
}

#[derive(Debug, Default)]
pub(super) struct Catalog {
    works: Vec<Entry>,
    assets: Vec<Entry>,
    cards: Vec<Entry>,
}

impl Catalog {
    pub(super) fn load(repo: &Repository) -> Result<Self> {
        let mut works: Vec<_> = repo.list_works()?.into_iter().map(from_work).collect();
        let mut assets: Vec<_> = repo.list_assets()?.into_iter().map(from_asset).collect();
        let mut cards: Vec<_> = repo.list_cards()?.into_iter().map(from_card).collect();
        for entries in [&mut works, &mut assets, &mut cards] {
            disambiguate(entries);
        }
        Ok(Self {
            works,
            assets,
            cards,
        })
    }

    pub(super) fn entries(&self, kind: Kind) -> &[Entry] {
        match kind {
            Kind::Work => &self.works,
            Kind::Asset => &self.assets,
            Kind::Card => &self.cards,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_collisions_add_ids_only_when_needed() {
        let mut items = vec![
            Entry::new("Same", "a".into(), String::new()),
            Entry::new("Different", "b".into(), String::new()),
            Entry::new("Same", "c".into(), String::new()),
        ];
        disambiguate(&mut items);
        assert_eq!(items[0].label, "Same [a]");
        assert_eq!(items[1].label, "Different");
        assert_eq!(items[2].label, "Same [c]");
    }

    #[test]
    fn malicious_control_bytes_are_filtered_from_rendered_content() {
        assert_eq!(terminal_safe("foo\u{1b}[31m\nbar", false), "foo[31m bar");
        assert_eq!(terminal_safe("a\t\nb\u{7f}", true), "a \nb");
    }
    #[test]
    fn existing_entity_fixtures_have_useful_details() {
        let work: Work =
            serde_json::from_str(include_str!("../../tests/fixtures/work.json")).unwrap();
        let asset: Asset =
            serde_json::from_str(include_str!("../../tests/fixtures/asset.json")).unwrap();
        let card: Card =
            serde_json::from_str(include_str!("../../tests/fixtures/card.json")).unwrap();
        assert!(from_work(work).detail.contains("Status:"));
        assert!(from_asset(asset).detail.contains("SHA-256:"));
        assert!(from_card(card).detail.contains("Evidence entries:"));
    }
}
