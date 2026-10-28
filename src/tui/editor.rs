//! External Markdown editor integration for Work and Card details.
//!
//! We reuse the CLI's editor bridge and the repository's optimistic concurrency
//! checks. The TUI must release raw mode and the alternate screen before calling
//! `edit_selected` (see `SuspendedTerminal`).

use super::catalog::Kind;
use crate::cli::editor::edit_markdown;
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use digital_notes::{
    model::{CardId, WorkId},
    repository::{Repository, Revision},
};
use std::{error::Error, io};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Leave the TUI modes before launching an interactive editor. If launching
/// the editor panics or fails, Drop makes a best-effort attempt to restore them.
pub(super) struct SuspendedTerminal {
    resumed: bool,
}

impl SuspendedTerminal {
    pub(super) fn new() -> io::Result<Self> {
        disable_raw_mode()?;
        let guard = Self { resumed: false };
        execute!(io::stdout(), LeaveAlternateScreen)?;
        Ok(guard)
    }

    pub(super) fn resume(mut self) -> io::Result<()> {
        execute!(io::stdout(), EnterAlternateScreen)?;
        enable_raw_mode()?;
        self.resumed = true;
        Ok(())
    }
}

impl Drop for SuspendedTerminal {
    fn drop(&mut self) {
        if !self.resumed {
            let _ = execute!(io::stdout(), EnterAlternateScreen);
            let _ = enable_raw_mode();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EditOutcome {
    Saved,
    Unchanged,
}

/// The revision and body are read together, BEFORE opening the editor.
/// Concurrent updates cannot be silently overwritten on save.
enum Snapshot {
    Work {
        id: WorkId,
        revision: Revision,
        body: String,
    },
    Card {
        id: CardId,
        revision: Revision,
        body: String,
    },
}

impl Snapshot {
    fn load(repo: &Repository, kind: Kind, id: &str) -> Result<Self> {
        match kind {
            Kind::Work => {
                let loaded = repo.load_work(&id.parse::<WorkId>()?)?;
                Ok(Self::Work {
                    id: loaded.value.id,
                    revision: loaded.revision,
                    body: loaded.value.body,
                })
            }
            Kind::Card => {
                let loaded = repo.load_card(&id.parse::<CardId>()?)?;
                Ok(Self::Card {
                    id: loaded.value.id,
                    revision: loaded.revision,
                    body: loaded.value.body,
                })
            }
            Kind::Asset => {
                Err(io::Error::other("Asset HTML is immutable and cannot be edited").into())
            }
        }
    }

    fn body(&self) -> &str {
        match self {
            Self::Work { body, .. } | Self::Card { body, .. } => body,
        }
    }

    fn save(self, repo: &Repository, text: &str) -> Result<EditOutcome> {
        if text == self.body() {
            return Ok(EditOutcome::Unchanged);
        }
        match self {
            Self::Work { id, revision, .. } => {
                repo.update_work(&id, &revision, |work| work.body = text.to_owned())?;
            }
            Self::Card { id, revision, .. } => {
                repo.update_card(&id, &revision, |card| card.body = text.to_owned())?;
            }
        }
        Ok(EditOutcome::Saved)
    }
}

/// Run only while the terminal has been suspended. If a save fails after a
/// successful editor exit, preserve the edited temp file for manual recovery.
pub(super) fn edit_selected(
    repo: &Repository,
    kind: Kind,
    id: &str,
) -> std::result::Result<EditOutcome, String> {
    let snapshot = Snapshot::load(repo, kind, id).map_err(|error| error.to_string())?;
    let edited = edit_markdown(snapshot.body()).map_err(|error| error.to_string())?;
    match snapshot.save(repo, &edited.text) {
        Ok(outcome) => Ok(outcome),
        Err(error) => match edited.preserve() {
            Ok(path) => Err(format!(
                "Save failed: {error}; edited Markdown preserved at {}",
                path.display()
            )),
            Err(preserve_error) => Err(format!(
                "Save failed: {error}; could not preserve draft: {preserve_error}"
            )),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_notes::model::{Card, Work};
    use tempfile::tempdir;

    fn repo_with_work_and_card() -> (tempfile::TempDir, Repository) {
        let temp = tempdir().unwrap();
        let repo = Repository::init(temp.path()).unwrap();
        let work: Work =
            serde_json::from_str(include_str!("../../tests/fixtures/work.json")).unwrap();
        repo.create_work(work).unwrap();
        let card: Card =
            serde_json::from_str(include_str!("../../tests/fixtures/card.json")).unwrap();
        repo.create_card(card).unwrap();
        (temp, repo)
    }

    #[test]
    fn work_and_card_save_only_markdown_body() {
        let (_temp, repo) = repo_with_work_and_card();
        for (kind, id, replacement) in [
            (Kind::Work, "work0001", "# Changed work\n"),
            (Kind::Card, "card0001", "# Changed card\n"),
        ] {
            let snapshot = Snapshot::load(&repo, kind, id).unwrap();
            assert_eq!(
                snapshot.save(&repo, replacement).unwrap(),
                EditOutcome::Saved
            );
            let current = Snapshot::load(&repo, kind, id).unwrap();
            assert_eq!(current.body(), replacement);
        }
        let work = repo.load_work(&"work0001".parse().unwrap()).unwrap().value;
        assert_eq!(work.title.as_str(), "Collect research papers");
        assert_eq!(work.status, digital_notes::model::WorkStatus::Active);
        let card = repo.load_card(&"card0001".parse().unwrap()).unwrap().value;
        assert_eq!(card.evidence.len(), 1);
        assert_eq!(card.source_asset_ids.len(), 2);
    }

    #[test]
    fn unchanged_body_does_not_bump_revision() {
        let (_temp, repo) = repo_with_work_and_card();
        let original = repo.load_work(&"work0001".parse().unwrap()).unwrap();
        let snapshot = Snapshot::load(&repo, Kind::Work, "work0001").unwrap();
        let body = snapshot.body().to_owned();
        assert_eq!(snapshot.save(&repo, &body).unwrap(), EditOutcome::Unchanged);
        let current = repo.load_work(&"work0001".parse().unwrap()).unwrap();
        assert_eq!(original.revision, current.revision);
    }

    #[test]
    fn stale_edit_does_not_overwrite_concurrent_change() {
        let (_temp, repo) = repo_with_work_and_card();
        for (kind, id) in [(Kind::Work, "work0001"), (Kind::Card, "card0001")] {
            let stale = Snapshot::load(&repo, kind, id).unwrap();
            let current = Snapshot::load(&repo, kind, id).unwrap();
            current.save(&repo, "Concurrent update").unwrap();
            let error = stale.save(&repo, "Stale draft").unwrap_err().to_string();
            assert!(error.contains("changed since last read"), "{error}");
            assert_eq!(
                Snapshot::load(&repo, kind, id).unwrap().body(),
                "Concurrent update"
            );
        }
    }

    #[test]
    fn rejects_asset_and_unknown_ids() {
        let (_temp, repo) = repo_with_work_and_card();
        assert!(Snapshot::load(&repo, Kind::Asset, "asset001").is_err());
        assert!(Snapshot::load(&repo, Kind::Work, "work9999").is_err());
    }
}
