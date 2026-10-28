//! Evidence registration wizard for the TUI.
//!
//! The UI collects a Card, an Asset, and an exact quote, then delegates
//! validation and atomic Card publication to Repository::add_card_evidence.

use super::{
    catalog::Kind,
    state::{Action, App, View},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use digital_notes::{
    model::{AssetId, CardId},
    repository::{Repository, Revision},
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Stage {
    Asset,
    Quote,
    Start,
    Confirm,
}

#[derive(Debug)]
pub(super) struct Form {
    pub(super) card_id: CardId,
    pub(super) revision: Revision,
    pub(super) stage: Stage,
    pub(super) selected_asset: usize,
    pub(super) quote: String,
    pub(super) start_text: String,
}

impl Form {
    fn new(card_id: CardId, revision: Revision) -> Self {
        Self {
            card_id,
            revision,
            stage: Stage::Asset,
            selected_asset: 0,
            quote: String::new(),
            start_text: String::new(),
        }
    }

    fn requested_start(&self) -> Result<Option<usize>, &'static str> {
        if self.start_text.is_empty() {
            Ok(None)
        } else {
            self.start_text
                .parse::<usize>()
                .map(Some)
                .map_err(|_| "Invalid start position")
        }
    }
}

impl App {
    pub(super) fn begin_evidence(&mut self, repo: &Repository) {
        if self.kind != Kind::Card || self.view != View::Detail {
            return;
        }
        let Some(id) = self.selected_entry().map(|entry| entry.id.clone()) else {
            return;
        };
        let card_id: CardId = match id.parse() {
            Ok(id) => id,
            Err(error) => {
                self.message = format!("Invalid Card ID: {error}");
                return;
            }
        };
        match repo.load_card(&card_id) {
            Ok(card) => {
                self.evidence_form = Some(Form::new(card_id, card.revision));
                self.view = View::Evidence;
                self.message.clear();
            }
            Err(error) => self.message = format!("Cannot open Card: {error}"),
        }
    }

    pub(super) fn evidence_key(&mut self, key: KeyEvent) -> Action {
        if key.code == KeyCode::Esc {
            self.evidence_form = None;
            self.view = View::Detail;
            self.message = "Evidence registration cancelled".into();
            return Action::Continue;
        }
        let Some(form) = self.evidence_form.as_mut() else {
            self.view = View::Detail;
            return Action::Continue;
        };
        match form.stage {
            Stage::Asset => {
                let count = self.catalog.entries(Kind::Asset).len();
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        form.selected_asset = form.selected_asset.saturating_sub(1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if count > 0 {
                            form.selected_asset = (form.selected_asset + 1).min(count - 1);
                        }
                    }
                    KeyCode::Enter if count > 0 => {
                        form.stage = Stage::Quote;
                        self.message.clear();
                    }
                    _ => {}
                }
            }
            Stage::Quote => match key.code {
                KeyCode::Backspace => {
                    form.quote.pop();
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    form.quote.clear();
                }
                KeyCode::Char(ch)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                        && !ch.is_control()
                        && form.quote.chars().count() < 4096 =>
                {
                    form.quote.push(ch);
                }
                KeyCode::Enter => {
                    if form.quote.trim().is_empty() {
                        self.message = "Enter a non-empty exact quotation".into();
                    } else {
                        form.stage = Stage::Start;
                        self.message.clear();
                    }
                }
                _ => {}
            },
            Stage::Start => match key.code {
                KeyCode::Backspace => {
                    form.start_text.pop();
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    form.start_text.clear();
                }
                KeyCode::Char(ch)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                        && ch.is_ascii_digit()
                        && form.start_text.len() < 20 =>
                {
                    form.start_text.push(ch);
                }
                KeyCode::Tab => {
                    form.stage = Stage::Quote;
                    self.message.clear();
                }
                KeyCode::Enter => {
                    if form.requested_start().is_ok() {
                        form.stage = Stage::Confirm;
                        self.message.clear();
                    } else {
                        self.message = "Invalid Unicode codepoint position".into();
                    }
                }
                _ => {}
            },
            Stage::Confirm => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => return Action::SaveEvidence,
                KeyCode::Backspace | KeyCode::Char('b') => {
                    form.stage = Stage::Start;
                    self.message.clear();
                }
                _ => {}
            },
        }
        Action::Continue
    }

    pub(super) fn save_evidence(&mut self, repo: &Repository) {
        let Some(form) = self.evidence_form.as_ref() else {
            return;
        };
        if form.stage != Stage::Confirm {
            return;
        }
        let Some(asset) = self.catalog.entries(Kind::Asset).get(form.selected_asset) else {
            self.message = "Selected Asset is no longer available".into();
            return;
        };
        let asset_id: AssetId = match asset.id.parse() {
            Ok(id) => id,
            Err(error) => {
                self.message = format!("Invalid Asset ID: {error}");
                return;
            }
        };
        let requested_start = match form.requested_start() {
            Ok(start) => start,
            Err(error) => {
                self.message = error.into();
                return;
            }
        };
        let result = repo.add_card_evidence(
            &form.card_id,
            &form.revision,
            &asset_id,
            &form.quote,
            requested_start,
            None, // append the marker to the Card body
        );
        match result {
            Ok(saved) => {
                let evidence_id = saved
                    .value
                    .evidence
                    .last()
                    .map(|entry| entry.id.to_string());
                self.evidence_form = None;
                self.view = View::Detail;
                self.refresh(repo);
                if let Some(id) = evidence_id {
                    if self.message == "Reloaded" {
                        self.message = format!("Evidence {id} saved; marker appended to Card");
                    } else {
                        self.message = format!("Evidence {id} saved; {}", self.message);
                    }
                }
            }
            Err(error) => {
                // Preserve the draft, so an ambiguous quote can be corrected;
                // stale revisions are rejected by the repository, never overwritten.
                self.message = format!("Evidence not saved: {error}");
            }
        }
    }
}

pub(super) fn draw(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(form) = &app.evidence_form else {
        frame.render_widget(Paragraph::new("No Evidence draft"), area);
        return;
    };
    match form.stage {
        Stage::Asset => {
            let items = app
                .catalog
                .entries(Kind::Asset)
                .iter()
                .map(|asset| ListItem::new(asset.label.as_str()))
                .collect::<Vec<_>>();
            if items.is_empty() {
                frame.render_widget(
                    Paragraph::new("No Assets available. Capture an Asset first.").block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title(" Evidence: choose Asset "),
                    ),
                    area,
                );
                return;
            }
            let list = List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Evidence: choose Asset "),
                )
                .highlight_symbol("› ")
                .highlight_style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                );
            let mut state = ListState::default();
            state.select(Some(form.selected_asset));
            frame.render_stateful_widget(list, area, &mut state);
        }
        Stage::Quote => {
            let label = app
                .catalog
                .entries(Kind::Asset)
                .get(form.selected_asset)
                .map_or("(missing)", |asset| asset.label.as_str());
            frame.render_widget(
                Paragraph::new(format!(
                    "Asset: {label}\n\nExact quotation:\n{}█",
                    form.quote
                ))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Evidence: quote "),
                )
                .wrap(Wrap { trim: false }),
                area,
            );
        }
        Stage::Start => {
            frame.render_widget(Paragraph::new(format!(
                "Optional Unicode codepoint start offset (leave blank to auto-find):\n\n{}█\n\nFor repeated quotes, enter the exact start position.",
                form.start_text,
            ))
                .block(Block::default().borders(Borders::ALL).title(" Evidence: position "))
                .wrap(Wrap { trim: false }), area);
        }
        Stage::Confirm => {
            let label = app
                .catalog
                .entries(Kind::Asset)
                .get(form.selected_asset)
                .map_or("(missing)", |asset| asset.label.as_str());
            let start = if form.start_text.is_empty() {
                "auto"
            } else {
                &form.start_text
            };
            frame.render_widget(Paragraph::new(format!(
                "Card: {}\nAsset: {label}\nStart: {start}\n\nQuote:\n{}\n\nAppend citation marker to Card? Press y to save.",
                form.card_id, form.quote,
            ))
                .block(Block::default().borders(Borders::ALL).title(" Evidence: confirm "))
                .wrap(Wrap { trim: false }), area);
        }
    }
}

pub(super) fn controls(app: &App) -> &'static str {
    match app.evidence_form.as_ref().map(|form| form.stage) {
        Some(Stage::Asset) => "↑↓ select Asset | Enter choose | Esc cancel",
        Some(Stage::Quote) => "Type quote | Enter next | Backspace delete | Esc cancel",
        Some(Stage::Start) => {
            "Type start or leave blank | Tab edit quote | Enter review | Esc cancel"
        }
        Some(Stage::Confirm) => "y save | b edit position | Esc cancel",
        None => "Esc cancel",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEventKind;
    use digital_notes::model::{Asset, Card, ContentHash, Work};
    use sha2::{Digest, Sha256};
    use tempfile::TempDir;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new_with_kind(code, KeyModifiers::NONE, KeyEventKind::Press)
    }

    fn fixture(html: &str) -> (TempDir, Repository, App) {
        let temp = tempfile::tempdir().unwrap();
        let repo = Repository::init(temp.path()).unwrap();
        let work: Work =
            serde_json::from_str(include_str!("../../tests/fixtures/work.json")).unwrap();
        repo.create_work(work).unwrap();
        let mut asset: Asset =
            serde_json::from_str(include_str!("../../tests/fixtures/asset.json")).unwrap();
        asset.related_work_ids.clear();
        asset.content_hash = format!("{:x}", Sha256::digest(html.as_bytes()))
            .parse::<ContentHash>()
            .unwrap();
        repo.create_asset_pair(asset, html.as_bytes()).unwrap();
        let mut card: Card =
            serde_json::from_str(include_str!("../../tests/fixtures/card.json")).unwrap();
        card.source_work_ids.clear();
        card.source_asset_ids.clear();
        card.evidence.clear();
        card.body.clear();
        repo.create_card(card).unwrap();
        let mut app = App::new(super::super::catalog::Catalog::load(&repo).unwrap());
        app.on_key(key(KeyCode::Char('3')));
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.view, View::Detail);
        assert_eq!(app.on_key(key(KeyCode::Char('e'))), Action::BeginEvidence);
        app.begin_evidence(&repo);
        assert_eq!(app.view, View::Evidence);
        (temp, repo, app)
    }

    fn text(app: &mut App, input: &str) {
        for character in input.chars() {
            assert_eq!(app.on_key(key(KeyCode::Char(character))), Action::Continue);
        }
    }

    fn to_confirmation(app: &mut App, quote: &str, start: &str) {
        app.on_key(key(KeyCode::Enter)); // Asset
        text(app, quote);
        app.on_key(key(KeyCode::Enter)); // Quote
        text(app, start);
        app.on_key(key(KeyCode::Enter)); // Offset
        assert_eq!(app.evidence_form.as_ref().unwrap().stage, Stage::Confirm);
    }

    #[test]
    fn saves_exact_quote_and_appends_marker_once() {
        let (_tmp, repo, mut app) = fixture("<p>Alpha citation Omega</p>");
        to_confirmation(&mut app, "citation", "");
        assert_eq!(app.on_key(key(KeyCode::Char('y'))), Action::SaveEvidence);
        app.save_evidence(&repo);
        assert_eq!(app.view, View::Detail);
        assert!(app.evidence_form.is_none());
        let card = repo.load_card(&"card0001".parse().unwrap()).unwrap().value;
        assert_eq!(card.evidence.len(), 1);
        assert_eq!(card.evidence[0].selector.exact, "citation");
        assert_eq!(card.evidence[0].position_selector.start, 6);
        assert_eq!(card.source_asset_ids.len(), 1);
        assert_eq!(card.body, "[^ev1]");
        assert!(
            app.selected_entry()
                .unwrap()
                .detail
                .contains("Evidence entries: 1")
        );
    }

    #[test]
    fn ambiguous_quote_keeps_draft_and_does_not_write() {
        let (_tmp, repo, mut app) = fixture("<p>target target</p>");
        let before = std::fs::read(repo.card_path(&"card0001".parse().unwrap())).unwrap();
        to_confirmation(&mut app, "target", "");
        app.save_evidence(&repo);
        assert_eq!(app.view, View::Evidence);
        assert!(app.message.contains("ambiguous"));
        assert_eq!(
            std::fs::read(repo.card_path(&"card0001".parse().unwrap())).unwrap(),
            before
        );
        app.on_key(key(KeyCode::Char('b')));
        assert_eq!(app.evidence_form.as_ref().unwrap().stage, Stage::Start);
        text(&mut app, "7");
        app.on_key(key(KeyCode::Enter));
        app.save_evidence(&repo);
        assert_eq!(app.view, View::Detail);
        assert_eq!(
            repo.load_card(&"card0001".parse().unwrap())
                .unwrap()
                .value
                .evidence[0]
                .position_selector
                .start,
            7
        );
    }

    #[test]
    fn concurrent_card_edit_is_not_overwritten() {
        let (_tmp, repo, mut app) = fixture("<p>citation</p>");
        to_confirmation(&mut app, "citation", "");
        let id = "card0001".parse().unwrap();
        let loaded = repo.load_card(&id).unwrap();
        repo.update_card(&id, &loaded.revision, |card| {
            card.body = "newer note".into()
        })
        .unwrap();
        app.save_evidence(&repo);
        assert!(app.message.contains("file changed since last read"));
        assert_eq!(app.view, View::Evidence);
        let card = repo.load_card(&id).unwrap().value;
        assert_eq!(card.body, "newer note");
        assert!(card.evidence.is_empty());
    }

    #[test]
    fn escape_cancels_without_writing_and_q_is_text() {
        let (_tmp, repo, mut app) = fixture("<p>q</p>");
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.on_key(key(KeyCode::Char('q'))), Action::Continue);
        assert_eq!(app.evidence_form.as_ref().unwrap().quote, "q");
        app.on_key(key(KeyCode::Esc));
        assert_eq!(app.view, View::Detail);
        assert!(app.evidence_form.is_none());
        assert!(
            repo.load_card(&"card0001".parse().unwrap())
                .unwrap()
                .value
                .evidence
                .is_empty()
        );
    }

    #[test]
    fn empty_repository_asset_list_does_not_advance() {
        let (_tmp, repo, mut app) = fixture("<p>source</p>");
        app.catalog = super::super::catalog::Catalog::default();
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.evidence_form.as_ref().unwrap().stage, Stage::Asset);
        app.on_key(key(KeyCode::Esc));
        assert_eq!(app.view, View::Detail);
        assert!(
            repo.load_card(&"card0001".parse().unwrap())
                .unwrap()
                .value
                .evidence
                .is_empty()
        );
    }
}
