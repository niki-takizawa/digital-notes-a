//! External Markdown editor bridge; no shell is invoked.
use std::{
    env,
    error::Error,
    fs,
    io::{self, Write},
    path::PathBuf,
    process::Command,
};
use tempfile::{Builder, NamedTempFile};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub struct EditedMarkdown {
    pub text: String,
    draft: NamedTempFile,
}

impl EditedMarkdown {
    /// Retain changes if an optimistic-concurrency save fails.
    pub fn preserve(self) -> Result<PathBuf> {
        let (_, path) = self.draft.keep()?;
        Ok(path)
    }
}

pub fn edit_markdown(body: &str) -> Result<EditedMarkdown> {
    let command = env::var("VISUAL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| env::var("EDITOR").ok().filter(|s| !s.trim().is_empty()))
        .ok_or_else(|| io::Error::other("set VISUAL or EDITOR to edit Markdown"))?;
    let mut argv = shell_words::split(&command)?;
    if argv.is_empty() {
        return Err(io::Error::other("VISUAL/EDITOR command is empty").into());
    }
    let mut file = Builder::new()
        .prefix("digital-notes-")
        .suffix(".md")
        .tempfile()?;
    file.write_all(body.as_bytes())?;
    file.flush()?;
    let executable = argv.remove(0);
    let outcome = Command::new(executable)
        .args(argv)
        .arg(file.path())
        .status();
    match outcome {
        Ok(status) if status.success() => {}
        Ok(status) => {
            let (_, path) = file.keep()?;
            return Err(io::Error::other(format!(
                "editor exited with {status}; Markdown preserved at {}",
                path.display()
            ))
            .into());
        }
        Err(error) => {
            let (_, path) = file.keep()?;
            return Err(io::Error::other(format!(
                "could not launch editor: {error}; Markdown preserved at {}",
                path.display()
            ))
            .into());
        }
    }
    let text = match fs::read_to_string(file.path()) {
        Ok(text) => text,
        Err(error) => {
            let (_, path) = file.keep()?;
            return Err(io::Error::other(format!(
                "could not read edited Markdown: {error}; draft preserved at {}",
                path.display()
            ))
            .into());
        }
    };
    Ok(EditedMarkdown { text, draft: file })
}
