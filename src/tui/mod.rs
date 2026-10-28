//! TUI entry point, sharing Repository APIs with the CLI.

mod catalog;
mod evidence;
mod render;
mod state;

use crate::tui::state::{Action, App};
use crossterm::event::{self, Event, KeyEventKind};
use digital_notes::repository::Repository;
use std::{
    error::Error,
    io::{self, IsTerminal},
};

pub(super) fn run(repo: &Repository) -> Result<(), Box<dyn Error>> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(
            io::Error::other("`digital-notes tui` requires an interactive terminal").into(),
        );
    }
    let mut app = App::new(catalog::Catalog::load(repo)?);
    // run restores raw mode and the alternate screen even if the loop errors.
    ratatui::run(|terminal| -> io::Result<()> {
        loop {
            terminal.draw(|frame| render::draw(frame, &app))?;
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => match app.on_key(key) {
                    Action::Quit => break Ok(()),
                    Action::Refresh => app.refresh(repo),
                    Action::BeginEvidence => app.begin_evidence(repo),
                    Action::SaveEvidence => app.save_evidence(repo),
                    Action::Continue => {}
                },
                _ => {}
            }
        }
    })?;
    Ok(())
}
