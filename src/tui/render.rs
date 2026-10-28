//! Rendering only; all navigation and data access live in other modules.

use super::state::{App, View};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Tabs, Wrap},
};

pub(super) fn draw(frame: &mut Frame<'_>, app: &App) {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(frame.area());

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

    match app.view {
        View::Evidence => super::evidence::draw(frame, app, sections[1]),
        View::Detail => {
            let detail = app
                .selected_entry()
                .map_or("No selection", |entry| entry.detail.as_str());
            frame.render_widget(
                Paragraph::new(detail)
                    .block(Block::default().borders(Borders::ALL).title(" Details "))
                    .wrap(Wrap { trim: false })
                    .scroll((app.scroll, 0)),
                sections[1],
            );
        }
        View::Related => {
            let relations = app.related();
            let items = relations
                .iter()
                .map(|item| ListItem::new(item.label.as_str()))
                .collect::<Vec<_>>();
            let list = List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Related entities (both directions) "),
                )
                .highlight_symbol("› ")
                .highlight_style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                );
            let mut state = ListState::default();
            state.select((app.related_selected < relations.len()).then_some(app.related_selected));
            frame.render_stateful_widget(list, sections[1], &mut state);
        }
        View::Browse => {
            let main = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
                .split(sections[1]);
            let matches = app.matches();
            let items = matches
                .iter()
                .map(|entry| ListItem::new(entry.label.as_str()))
                .collect::<Vec<_>>();
            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title(format!(
                    " {} ({}) ",
                    app.kind.name(),
                    matches.len()
                )))
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
                .map_or("No matching entries", |entry| entry.detail.as_str());
            frame.render_widget(
                Paragraph::new(preview)
                    .block(Block::default().borders(Borders::ALL).title(" Preview "))
                    .wrap(Wrap { trim: false }),
                main[1],
            );
        }
    }

    let hint = if app.view == View::Evidence {
        super::evidence::controls(app).to_owned()
    } else if let Some(draft) = &app.search_draft {
        format!("/{draft}  | Enter search · Esc cancel · Backspace delete")
    } else {
        let controls = match app.view {
            View::Browse => "1/2/3 Tab kind | ↑↓ select | Enter details",
            View::Detail if app.kind == super::catalog::Kind::Card => {
                "↑↓ scroll | g relations | e evidence | b back | Esc list"
            }
            View::Detail => "↑↓ scroll | g relations | b back | Esc list",
            View::Related => "↑↓ choose | Enter follow | Esc details",
            View::Evidence => super::evidence::controls(app),
        };
        let search = if app.query.is_empty() {
            String::new()
        } else {
            format!(" | search: {}", app.query)
        };
        format!("q quit | / search | r reload | {controls}{search}")
    };
    let footer = if app.message.is_empty() {
        hint
    } else {
        // Repository errors may contain paths: remove terminal controls.
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
    fn all_views_render_on_small_terminals() {
        let mut app = App::new(Catalog::default());
        for view in [View::Browse, View::Detail, View::Related] {
            app.view = view;
            for (width, height) in [(80, 24), (30, 8), (8, 3)] {
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal.draw(|frame| draw(frame, &app)).unwrap();
            }
        }
        app.search_draft = Some("research".into());
        let mut terminal = Terminal::new(TestBackend::new(30, 8)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
    }
}
