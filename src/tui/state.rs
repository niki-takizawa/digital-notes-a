//! Terminal-independent navigation, search, and related-entity selection.

use super::catalog::{Catalog, Entry, Kind, Related};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use digital_notes::repository::Repository;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Continue,
    Refresh,
    BeginEvidence,
    SaveEvidence,
    EditMarkdown,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum View {
    Browse,
    Detail,
    Related,
    Evidence,
}

#[derive(Debug)]
pub(super) struct App {
    pub(super) catalog: Catalog,
    pub(super) kind: Kind,
    selected: [usize; 3],
    pub(super) view: View,
    pub(super) scroll: u16,
    pub(super) message: String,
    pub(super) query: String,
    pub(super) search_draft: Option<String>,
    pub(super) related_selected: usize,
    history: Vec<(Kind, String)>,
    pub(super) evidence_form: Option<super::evidence::Form>,
}

impl App {
    pub(super) fn new(catalog: Catalog) -> Self {
        Self {
            catalog,
            kind: Kind::Work,
            selected: [0; 3],
            view: View::Browse,
            scroll: 0,
            message: String::new(),
            query: String::new(),
            search_draft: None,
            related_selected: 0,
            history: Vec::new(),
            evidence_form: None,
        }
    }

    /// Search is per kind, but the query stays active when switching tabs.
    pub(super) fn matches(&self) -> Vec<&Entry> {
        let query = self.query.to_lowercase();
        self.catalog
            .entries(self.kind)
            .iter()
            .filter(|entry| query.is_empty() || entry.detail.to_lowercase().contains(&query))
            .collect()
    }

    pub(super) fn selected_index(&self) -> Option<usize> {
        let index = self.selected[self.kind.index()];
        (index < self.matches().len()).then_some(index)
    }

    pub(super) fn selected_entry(&self) -> Option<&Entry> {
        let index = self.selected_index()?;
        self.matches().get(index).copied()
    }

    pub(super) fn related(&self) -> Vec<Related> {
        self.selected_entry()
            .map_or_else(Vec::new, |entry| self.catalog.related(self.kind, &entry.id))
    }

    fn select_kind(&mut self, kind: Kind) {
        self.kind = kind;
        self.selected[kind.index()] = 0;
        self.view = View::Browse;
        self.scroll = 0;
        self.history.clear();
        self.message.clear();
    }

    fn handle_search(&mut self, key: KeyEvent) {
        let Some(draft) = self.search_draft.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => {
                self.search_draft = None;
            }
            KeyCode::Enter => {
                self.query = draft.clone();
                self.search_draft = None;
                self.selected = [0; 3];
                self.view = View::Browse;
                self.history.clear();
                self.message.clear();
            }
            KeyCode::Backspace => {
                draft.pop();
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => draft.clear(),
            KeyCode::Char(ch)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                    && !ch.is_control() =>
            {
                draft.push(ch);
            }
            _ => {}
        }
    }

    pub(super) fn on_key(&mut self, key: KeyEvent) -> Action {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Action::Quit;
        }
        if self.view == View::Evidence {
            return self.evidence_key(key);
        }
        if self.search_draft.is_some() {
            self.handle_search(key);
            return Action::Continue;
        }
        match key.code {
            KeyCode::Char('q') => Action::Quit,
            KeyCode::Char('r') => Action::Refresh,
            KeyCode::Char('/') => {
                self.search_draft = Some(self.query.clone());
                Action::Continue
            }
            KeyCode::Esc => {
                self.view = match self.view {
                    View::Related | View::Evidence => View::Detail,
                    _ => View::Browse,
                };
                self.scroll = 0;
                Action::Continue
            }
            KeyCode::Tab => {
                self.select_kind(Kind::from_index(self.kind.index() + 1));
                Action::Continue
            }
            KeyCode::BackTab => {
                self.select_kind(Kind::from_index(self.kind.index() + 2));
                Action::Continue
            }
            KeyCode::Char('1' | '2' | '3') => {
                let index = match key.code {
                    KeyCode::Char('1') => 0,
                    KeyCode::Char('2') => 1,
                    _ => 2,
                };
                self.select_kind(Kind::from_index(index));
                Action::Continue
            }
            KeyCode::Enter => {
                match self.view {
                    View::Browse if self.selected_entry().is_some() => {
                        self.view = View::Detail;
                        self.scroll = 0;
                    }
                    View::Related => self.follow_related(),
                    _ => {}
                }
                Action::Continue
            }
            KeyCode::Char('m') if self.view == View::Detail && self.kind != Kind::Asset => {
                Action::EditMarkdown
            }
            KeyCode::Char('e') if self.view == View::Detail && self.kind == Kind::Card => {
                Action::BeginEvidence
            }
            KeyCode::Char('g') if self.view == View::Detail => {
                self.view = View::Related;
                self.related_selected = 0;
                Action::Continue
            }
            KeyCode::Char('b') | KeyCode::Backspace if self.view == View::Detail => {
                self.go_back();
                Action::Continue
            }
            KeyCode::Down | KeyCode::Char('j') => {
                match self.view {
                    View::Detail => self.scroll = self.scroll.saturating_add(1),
                    View::Evidence => {}
                    View::Related => {
                        let count = self.related().len();
                        if count > 0 {
                            self.related_selected = (self.related_selected + 1).min(count - 1);
                        }
                    }
                    View::Browse => {
                        let count = self.matches().len();
                        if count > 0 {
                            let index = &mut self.selected[self.kind.index()];
                            *index = (*index + 1).min(count - 1);
                        }
                    }
                }
                Action::Continue
            }
            KeyCode::Up | KeyCode::Char('k') => {
                match self.view {
                    View::Detail => self.scroll = self.scroll.saturating_sub(1),
                    View::Evidence => {}
                    View::Related => {
                        self.related_selected = self.related_selected.saturating_sub(1)
                    }
                    View::Browse => {
                        let index = &mut self.selected[self.kind.index()];
                        *index = index.saturating_sub(1);
                    }
                }
                Action::Continue
            }
            _ => Action::Continue,
        }
    }

    fn follow_related(&mut self) {
        let Some(target) = self.related().get(self.related_selected).cloned() else {
            return;
        };
        if !target.exists {
            self.message = format!("Missing target: {}", target.id);
            return;
        }
        let origin_id = self.selected_entry().map(|entry| entry.id.clone());
        if let Some(id) = origin_id {
            self.history.push((self.kind, id));
        }
        self.jump_to(target.kind, &target.id);
    }

    fn jump_to(&mut self, kind: Kind, id: &str) {
        // Clear the active filter so that the destination is always selectable.
        self.query.clear();
        self.search_draft = None;
        self.kind = kind;
        self.selected[kind.index()] = self
            .catalog
            .entries(kind)
            .iter()
            .position(|entry| entry.id == id)
            .unwrap_or(0);
        self.view = View::Detail;
        self.scroll = 0;
        self.message.clear();
    }

    fn go_back(&mut self) {
        if let Some((kind, id)) = self.history.pop()
            && self.catalog.find(kind, &id).is_some()
        {
            self.jump_to(kind, &id);
        }
    }

    pub(super) fn finish_edit(
        &mut self,
        repo: &Repository,
        kind: Kind,
        id: &str,
        outcome: super::editor::EditOutcome,
    ) {
        if outcome == super::editor::EditOutcome::Unchanged {
            self.message = "Markdown unchanged".into();
            return;
        }
        match Catalog::load(repo) {
            Ok(catalog) => {
                self.catalog = catalog;
                self.jump_to(kind, id);
                self.message = "Markdown saved".into();
            }
            Err(error) => {
                self.message = format!("Markdown saved; catalog refresh failed: {error}");
            }
        }
    }

    pub(super) fn refresh(&mut self, repo: &Repository) {
        let old_id = self.selected_entry().map(|entry| entry.id.clone());
        match Catalog::load(repo) {
            Ok(catalog) => {
                self.catalog = catalog;
                self.history.clear();
                let retained_index =
                    old_id.and_then(|id| self.matches().iter().position(|entry| entry.id == id));
                if let Some(index) = retained_index {
                    self.selected[self.kind.index()] = index;
                }
                for kind in [Kind::Work, Kind::Asset, Kind::Card] {
                    if kind != self.kind {
                        self.selected[kind.index()] = 0;
                    }
                }
                self.selected[self.kind.index()] =
                    self.selected[self.kind.index()].min(self.matches().len().saturating_sub(1));
                if self.selected_entry().is_none() {
                    self.view = View::Browse;
                }
                if self.view == View::Related {
                    self.related_selected = 0;
                }
                self.scroll = 0;
                self.message = "Reloaded".into();
            }
            Err(error) => self.message = format!("Refresh failed: {error}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEventKind;
    use digital_notes::model::{Asset, Card, Work};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new_with_kind(code, KeyModifiers::NONE, KeyEventKind::Press)
    }

    fn fixtures() -> Catalog {
        Catalog::from_models(
            vec![
                serde_json::from_str::<Work>(include_str!("../../tests/fixtures/work.json"))
                    .unwrap(),
            ],
            vec![
                serde_json::from_str::<Asset>(include_str!("../../tests/fixtures/asset.json"))
                    .unwrap(),
            ],
            vec![
                serde_json::from_str::<Card>(include_str!("../../tests/fixtures/card.json"))
                    .unwrap(),
            ],
        )
    }

    #[test]
    fn empty_catalog_does_not_open_details_or_panic() {
        let mut app = App::new(Catalog::default());
        for code in [
            KeyCode::Down,
            KeyCode::Up,
            KeyCode::Enter,
            KeyCode::Esc,
            KeyCode::Char('g'),
        ] {
            assert_eq!(app.on_key(key(code)), Action::Continue);
        }
        assert_eq!(app.selected_index(), None);
        assert_eq!(app.view, View::Browse);
    }

    #[test]
    fn tab_selection_is_stable_between_kinds() {
        let mut app = App::new(Catalog::default());
        app.on_key(key(KeyCode::Char('2')));
        assert_eq!(app.kind, Kind::Asset);
        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.kind, Kind::Card);
        app.on_key(key(KeyCode::BackTab));
        assert_eq!(app.kind, Kind::Asset);
        assert_eq!(app.on_key(key(KeyCode::Char('q'))), Action::Quit);
    }

    #[test]
    fn search_filters_body_and_escape_cancels_edits() {
        let mut app = App::new(fixtures());
        app.on_key(key(KeyCode::Char('/')));
        for ch in "strategy".chars() {
            app.on_key(key(KeyCode::Char(ch)));
        }
        // q is ordinary text while search input is active.
        app.on_key(key(KeyCode::Char('q')));
        assert!(app.search_draft.is_some());
        app.on_key(key(KeyCode::Backspace));
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.matches().len(), 1);
        assert_eq!(app.query, "strategy");
        app.on_key(key(KeyCode::Char('/')));
        app.on_key(key(KeyCode::Char('x')));
        app.on_key(key(KeyCode::Esc));
        assert_eq!(app.query, "strategy");
        app.on_key(key(KeyCode::Tab));
        assert!(app.matches().is_empty());
        app.on_key(key(KeyCode::Char('/')));
        app.on_key(key(KeyCode::Char('u'))); // append, not Ctrl+U
        app.on_key(key(KeyCode::Backspace));
    }

    #[test]
    fn relations_navigate_across_kinds_and_back() {
        let mut app = App::new(fixtures());
        app.on_key(key(KeyCode::Enter));
        app.on_key(key(KeyCode::Char('g')));
        assert_eq!(app.view, View::Related);
        let related = app.related();
        assert!(
            related
                .iter()
                .any(|item| item.kind == Kind::Asset && item.exists)
        );
        let index = related
            .iter()
            .position(|item| item.kind == Kind::Asset && item.exists)
            .unwrap();
        app.related_selected = index;
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.kind, Kind::Asset);
        assert_eq!(app.view, View::Detail);
        app.on_key(key(KeyCode::Char('b')));
        assert_eq!(app.kind, Kind::Work);
        assert_eq!(app.view, View::Detail);
    }

    #[test]
    fn missing_related_target_is_reported_without_navigation() {
        let mut app = App::new(fixtures());
        app.on_key(key(KeyCode::Char('3')));
        app.on_key(key(KeyCode::Enter));
        app.on_key(key(KeyCode::Char('g')));
        let index = app.related().iter().position(|r| !r.exists).unwrap();
        app.related_selected = index;
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.kind, Kind::Card);
        assert_eq!(app.view, View::Related);
        assert!(app.message.contains("Missing target"));
    }

    #[test]
    fn markdown_edit_is_only_available_for_work_and_card_details() {
        let mut app = App::new(fixtures());
        assert_eq!(app.on_key(key(KeyCode::Char('m'))), Action::Continue);
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.on_key(key(KeyCode::Char('m'))), Action::EditMarkdown);
        app.on_key(key(KeyCode::Char('2')));
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.on_key(key(KeyCode::Char('m'))), Action::Continue);
        app.on_key(key(KeyCode::Char('3')));
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.on_key(key(KeyCode::Char('m'))), Action::EditMarkdown);
        assert_eq!(app.on_key(key(KeyCode::Char('e'))), Action::BeginEvidence);
    }

    #[test]
    fn refresh_key_requests_reload_without_io() {
        let mut app = App::new(Catalog::default());
        assert_eq!(app.on_key(key(KeyCode::Char('r'))), Action::Refresh);
    }
}
