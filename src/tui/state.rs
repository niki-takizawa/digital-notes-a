//! TUI navigation state, deliberately independent of terminal I/O.

use super::catalog::{Catalog, Entry, Kind};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use digital_notes::repository::Repository;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Continue,
    Refresh,
    Quit,
}

#[derive(Debug)]
pub(super) struct App {
    pub(super) catalog: Catalog,
    pub(super) kind: Kind,
    selected: [usize; 3],
    pub(super) detail_open: bool,
    pub(super) scroll: u16,
    pub(super) message: String,
}

impl App {
    pub(super) fn new(catalog: Catalog) -> Self {
        Self {
            catalog,
            kind: Kind::Work,
            selected: [0; 3],
            detail_open: false,
            scroll: 0,
            message: String::new(),
        }
    }

    pub(super) fn selected_index(&self) -> Option<usize> {
        let index = self.selected[self.kind.index()];
        (index < self.catalog.entries(self.kind).len()).then_some(index)
    }

    pub(super) fn selected_entry(&self) -> Option<&Entry> {
        self.selected_index()
            .map(|index| &self.catalog.entries(self.kind)[index])
    }

    fn select_kind(&mut self, kind: Kind) {
        self.kind = kind;
        self.detail_open = false;
        self.scroll = 0;
        self.message.clear();
    }

    pub(super) fn on_key(&mut self, key: KeyEvent) -> Action {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Action::Quit;
        }
        match key.code {
            KeyCode::Char('q') => Action::Quit,
            KeyCode::Char('r') => Action::Refresh,
            KeyCode::Esc => {
                self.detail_open = false;
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
            KeyCode::Enter if !self.detail_open => {
                if self.selected_entry().is_some() {
                    self.detail_open = true;
                    self.scroll = 0;
                }
                Action::Continue
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.detail_open {
                    self.scroll = self.scroll.saturating_add(1);
                } else {
                    let count = self.catalog.entries(self.kind).len();
                    if count > 0 {
                        let index = &mut self.selected[self.kind.index()];
                        *index = (*index + 1).min(count - 1);
                    }
                }
                Action::Continue
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.detail_open {
                    self.scroll = self.scroll.saturating_sub(1);
                } else {
                    let index = &mut self.selected[self.kind.index()];
                    *index = index.saturating_sub(1);
                }
                Action::Continue
            }
            _ => Action::Continue,
        }
    }

    pub(super) fn refresh(&mut self, repo: &Repository) {
        match Catalog::load(repo) {
            Ok(catalog) => {
                self.catalog = catalog;
                for kind in [Kind::Work, Kind::Asset, Kind::Card] {
                    let index = kind.index();
                    self.selected[index] = self.selected[index]
                        .min(self.catalog.entries(kind).len().saturating_sub(1));
                }
                if self.selected_entry().is_none() {
                    self.detail_open = false;
                }
                self.scroll = 0;
                self.message = "Reloaded".into();
            }
            Err(error) => {
                self.message = format!("Refresh failed: {error}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEventKind;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new_with_kind(code, KeyModifiers::NONE, KeyEventKind::Press)
    }

    #[test]
    fn empty_catalog_does_not_open_details_or_panic() {
        let mut app = App::new(Catalog::default());
        for code in [KeyCode::Down, KeyCode::Up, KeyCode::Enter, KeyCode::Esc] {
            assert_eq!(app.on_key(key(code)), Action::Continue);
        }
        assert_eq!(app.selected_index(), None);
        assert!(!app.detail_open);
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
    fn refresh_key_requests_reload_without_io() {
        let mut app = App::new(Catalog::default());
        assert_eq!(app.on_key(key(KeyCode::Char('r'))), Action::Refresh);
    }
}
