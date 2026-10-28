//! Read-only snapshots for the first TUI viewer.
//! Every displayed value is escaped for safe terminal rendering.

use digital_notes::{
    model::{Asset, Card, Work, WorkStatus},
    repository::{Repository, Result},
};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
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
    pub(super) references: Vec<Reference>,
}

#[derive(Clone, Debug)]
pub(super) struct Reference {
    pub(super) kind: Kind,
    pub(super) id: String,
    pub(super) role: &'static str,
}

impl Reference {
    fn new(kind: Kind, id: impl ToString, role: &'static str) -> Self {
        Self {
            kind,
            id: id.to_string(),
            role,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct Related {
    pub(super) kind: Kind,
    pub(super) id: String,
    pub(super) label: String,
    pub(super) exists: bool,
}

impl Entry {
    fn new(title: &str, id: String, detail: String) -> Self {
        let title = terminal_safe(title, false);
        Self {
            label: title.clone(),
            title,
            id,
            detail,
            references: Vec::new(),
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
    let mut entry = Entry::new(work.title.as_str(), id, terminal_safe(&detail, true));
    entry.references = work
        .source_work_ids
        .iter()
        .map(|id| Reference::new(Kind::Work, id, "Source Work"))
        .collect();
    entry
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
    let mut entry = Entry::new(asset.title.as_str(), id, terminal_safe(&detail, true));
    entry.references.push(Reference::new(
        Kind::Work,
        &asset.created_by_work_id,
        "Created by Work",
    ));
    entry.references.extend(
        asset
            .related_work_ids
            .iter()
            .map(|id| Reference::new(Kind::Work, id, "Related Work")),
    );
    entry
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
    let mut entry = Entry::new(card.title.as_str(), id, terminal_safe(&detail, true));
    entry.references.push(Reference::new(
        Kind::Work,
        &card.created_by_work_id,
        "Created by Work",
    ));
    entry.references.extend(
        card.source_work_ids
            .iter()
            .map(|id| Reference::new(Kind::Work, id, "Source Work")),
    );
    entry.references.extend(
        card.source_asset_ids
            .iter()
            .map(|id| Reference::new(Kind::Asset, id, "Source Asset")),
    );
    entry.references.extend(
        card.source_card_ids
            .iter()
            .map(|id| Reference::new(Kind::Card, id, "Source Card")),
    );
    // An Evidence's Asset is also a relationship even if source_asset_ids is missing.
    entry.references.extend(
        card.evidence
            .iter()
            .map(|ev| Reference::new(Kind::Asset, &ev.asset_id, "Evidence Asset")),
    );
    entry
}

#[derive(Debug, Default)]
pub(super) struct Catalog {
    works: Vec<Entry>,
    assets: Vec<Entry>,
    cards: Vec<Entry>,
}

impl Catalog {
    pub(super) fn load(repo: &Repository) -> Result<Self> {
        Ok(Self::from_models(
            repo.list_works()?,
            repo.list_assets()?,
            repo.list_cards()?,
        ))
    }

    pub(super) fn from_models(works: Vec<Work>, assets: Vec<Asset>, cards: Vec<Card>) -> Self {
        let mut works: Vec<_> = works.into_iter().map(from_work).collect();
        let mut assets: Vec<_> = assets.into_iter().map(from_asset).collect();
        let mut cards: Vec<_> = cards.into_iter().map(from_card).collect();
        for entries in [&mut works, &mut assets, &mut cards] {
            disambiguate(entries);
        }
        Self {
            works,
            assets,
            cards,
        }
    }

    pub(super) fn entries(&self, kind: Kind) -> &[Entry] {
        match kind {
            Kind::Work => &self.works,
            Kind::Asset => &self.assets,
            Kind::Card => &self.cards,
        }
    }

    pub(super) fn find(&self, kind: Kind, id: &str) -> Option<&Entry> {
        self.entries(kind).iter().find(|entry| entry.id == id)
    }

    /// List both outbound references and other entities that point back here.
    /// Missing targets remain visible but cannot be opened.
    pub(super) fn related(&self, kind: Kind, id: &str) -> Vec<Related> {
        let Some(entry) = self.find(kind, id) else {
            return Vec::new();
        };
        let mut result = Vec::new();
        let mut seen = HashSet::new();
        for reference in &entry.references {
            if (reference.kind, reference.id.as_str()) == (kind, id) {
                continue;
            }
            if seen.insert((reference.kind, reference.id.clone())) {
                result.push(self.make_related(reference.kind, &reference.id, reference.role));
            }
        }
        for source_kind in [Kind::Work, Kind::Asset, Kind::Card] {
            for source in self.entries(source_kind) {
                if source_kind == kind && source.id == id {
                    continue;
                }
                if source
                    .references
                    .iter()
                    .any(|reference| reference.kind == kind && reference.id == id)
                    && seen.insert((source_kind, source.id.clone()))
                {
                    result.push(self.make_related(source_kind, &source.id, "Referenced by"));
                }
            }
        }
        result
    }

    fn make_related(&self, kind: Kind, id: &str, role: &str) -> Related {
        let target = self.find(kind, id);
        let name = kind.name().trim_end_matches('s');
        let label = match target {
            Some(entry) => format!("{role} · {name}: {}", entry.label),
            None => format!("{role} · {name}: [missing: {id}]"),
        };
        Related {
            kind,
            id: id.to_owned(),
            label,
            exists: target.is_some(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relations_include_reverse_links_and_missing_targets() {
        let work: Work =
            serde_json::from_str(include_str!("../../tests/fixtures/work.json")).unwrap();
        let asset: Asset =
            serde_json::from_str(include_str!("../../tests/fixtures/asset.json")).unwrap();
        let card: Card =
            serde_json::from_str(include_str!("../../tests/fixtures/card.json")).unwrap();
        let catalog = Catalog::from_models(vec![work], vec![asset], vec![card]);
        let work_relations = catalog.related(Kind::Work, "work0001");
        assert!(
            work_relations
                .iter()
                .any(|related| related.kind == Kind::Asset
                    && related.label.contains("Example publication")
                    && related.exists)
        );
        assert!(
            work_relations
                .iter()
                .any(|related| related.kind == Kind::Card
                    && related.label.contains("Research notes")
                    && related.exists)
        );
        let asset_relations = catalog.related(Kind::Asset, "asset001");
        assert_eq!(
            asset_relations
                .iter()
                .filter(|related| related.kind == Kind::Card)
                .count(),
            1
        );
        assert!(
            catalog
                .related(Kind::Card, "card0001")
                .iter()
                .any(|related| !related.exists && related.id == "asset002")
        );
    }

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
