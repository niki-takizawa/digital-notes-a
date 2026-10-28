//! Rendering only. Navigation state and disk I/O live in other modules.

use super::state::App;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Tabs, Wrap},
};

pub(super) fn draw(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(area);

    let tabs = Tabs::new(["Works", "Assets", "Cards"])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" digital-notes "),
        )
        .select(app.kind.index())
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_widget(tabs, sections[0]);

    if app.detail_open {
        let detail = app
            .selected_entry()
            .map_or("No selection", |entry| entry.detail.as_str());
        let panel = Paragraph::new(detail)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Details (Esc to return) "),
            )
            .wrap(Wrap { trim: false })
            .scroll((app.scroll, 0));
        frame.render_widget(panel, sections[1]);
    } else {
        let main = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
            .split(sections[1]);
        let items = app
            .catalog
            .entries(app.kind)
            .iter()
            .map(|entry| ListItem::new(entry.label.as_str()))
            .collect::<Vec<_>>();
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" {} ", app.kind.name())),
            )
            .highlight_symbol("› ")
            .highlight_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );
        let mut state = ListState::default();
        state.select(app.selected_index());
        frame.render_stateful_widget(list, main[0], &mut state);

        let preview = app
            .selected_entry()
            .map_or("No entries yet", |entry| entry.detail.as_str());
        frame.render_widget(
            Paragraph::new(preview)
                .block(Block::default().borders(Borders::ALL).title(" Preview "))
                .wrap(Wrap { trim: false }),
            main[1],
        );
    }

    let hint = if app.detail_open {
        "q quit | ↑↓/jk scroll | Esc back | Tab switch | r reload"
    } else {
        "q quit | 1/2/3 or Tab switch | ↑↓/jk select | Enter details | r reload"
    };
    let footer = if app.message.is_empty() {
        hint.to_owned()
    } else {
        // Errors are generated from local paths and must also be terminal-safe.
        format!(
            "{hint} | {}",
            app.message
                .chars()
                .filter(|c| !c.is_control())
                .collect::<String>()
        )
    };
    frame.render_widget(
        Paragraph::new(footer).wrap(Wrap { trim: true }),
        sections[2],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::catalog::Catalog;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn empty_repositories_render_at_standard_and_small_terminal_sizes() {
        let app = App::new(Catalog::default());
        for (width, height) in [(80, 24), (30, 8), (8, 3)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| draw(frame, &app)).unwrap();
        }
    }
}
