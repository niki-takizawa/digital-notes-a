//! File-backed repository layout and storage primitives.
//!
//! Writers must use `Repository::begin_write` and keep the guard throughout
//! validation and publication. Card operations are added in a later commit.

mod asset;
mod capture;
mod card;
mod evidence;
mod references;
mod storage;
mod work;

use crate::model::{AssetId, CardId, WorkId};
use std::{
    error::Error as StdError,
    fmt, fs,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

pub use capture::{CaptureOutcome, CaptureRequest};
pub use references::{IntegrityIssue, IntegrityIssueKind};
pub use storage::{LoadedBytes, Revision, WriteGuard};

/// A decoded entity together with the revision required for safe updates.
#[derive(Debug)]
pub struct Loaded<T> {
    pub value: T,
    pub revision: Revision,
}

pub type Result<T> = std::result::Result<T, RepositoryError>;

#[derive(Debug)]
pub enum RepositoryError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    NotFound(PathBuf),
    AlreadyExists(PathBuf),
    InvalidLayout(PathBuf),
    InvalidPath(PathBuf),
    Conflict(PathBuf),
    InvalidAsset(String),
    InvalidWork(String),
    InvalidCard(String),
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
}

impl fmt::Display for RepositoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::NotFound(path) => write!(f, "file not found: {}", path.display()),
            Self::AlreadyExists(path) => write!(f, "file already exists: {}", path.display()),
            Self::InvalidLayout(path) => write!(f, "invalid repository layout: {}", path.display()),
            Self::InvalidPath(path) => write!(f, "invalid managed path: {}", path.display()),
            Self::Conflict(path) => write!(f, "file changed since last read: {}", path.display()),
            Self::InvalidAsset(message) => write!(f, "invalid Asset: {message}"),
            Self::InvalidWork(message) => write!(f, "invalid Work: {message}"),
            Self::InvalidCard(message) => write!(f, "invalid Card: {message}"),
            Self::Json { path, source } => {
                write!(f, "invalid JSON at {}: {source}", path.display())
            }
        }
    }
}

impl StdError for RepositoryError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Json { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn io_at(path: &Path, source: io::Error) -> RepositoryError {
    if source.kind() == ErrorKind::NotFound {
        RepositoryError::NotFound(path.to_owned())
    } else {
        RepositoryError::Io {
            path: path.to_owned(),
            source,
        }
    }
}

const SUBDIRS: [&str; 4] = [".pri", "works", "assets", "cards"];

#[derive(Clone, Debug)]
pub struct Repository {
    root: PathBuf,
}

impl Repository {
    /// Create missing directories without replacing existing entries.
    pub fn init(root: &Path) -> Result<Self> {
        fs::create_dir_all(root).map_err(|err| io_at(root, err))?;
        let root = fs::canonicalize(root).map_err(|err| io_at(root, err))?;
        for name in SUBDIRS {
            let path = root.join(name);
            match fs::create_dir(&path) {
                Ok(()) => {}
                Err(err) if err.kind() == ErrorKind::AlreadyExists => {}
                Err(err) => return Err(io_at(&path, err)),
            }
        }
        Self::open(&root)
    }

    /// Open an initialized repository; never follow symlinks for managed directories.
    pub fn open(root: &Path) -> Result<Self> {
        let root = fs::canonicalize(root).map_err(|err| io_at(root, err))?;
        for name in SUBDIRS {
            let path = root.join(name);
            let metadata = fs::symlink_metadata(&path).map_err(|err| io_at(&path, err))?;
            if !metadata.file_type().is_dir() {
                return Err(RepositoryError::InvalidLayout(path));
            }
        }
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn work_path(&self, id: &WorkId) -> PathBuf {
        self.root.join("works").join(format!("{id}.json"))
    }

    pub fn card_path(&self, id: &CardId) -> PathBuf {
        self.root.join("cards").join(format!("{id}.json"))
    }

    pub fn asset_path(&self, id: &AssetId) -> PathBuf {
        self.root.join("assets").join(format!("{id}.json"))
    }

    pub fn html_path(&self, id: &AssetId) -> PathBuf {
        self.root.join("assets").join(format!("{id}.html"))
    }

    /// Read a regular managed file and capture an exact, opaque revision token.
    /// Readers do not need a lock: publishing always replaces whole files.
    pub fn read(&self, path: &Path) -> Result<LoadedBytes> {
        self.validate_path(path)?;
        storage::read(path)
    }

    /// Acquire an exclusive advisory lock at `.pri/write.lock`.
    pub fn begin_write(&self) -> Result<WriteGuard<'_>> {
        storage::begin_write(self)
    }

    fn validate_path(&self, path: &Path) -> Result<()> {
        let parent = path.parent();
        let stem = path.file_stem().and_then(|s| s.to_str());
        let ext = path.extension().and_then(|s| s.to_str());
        let valid = match parent {
            Some(dir) if dir == self.root.join("works") => {
                ext == Some("json") && stem.is_some_and(|s| s.parse::<WorkId>().is_ok())
            }
            Some(dir) if dir == self.root.join("cards") => {
                ext == Some("json") && stem.is_some_and(|s| s.parse::<CardId>().is_ok())
            }
            Some(dir) if dir == self.root.join("assets") => {
                matches!(ext, Some("json" | "html"))
                    && stem.is_some_and(|s| s.parse::<AssetId>().is_ok())
            }
            _ => false,
        };
        if valid {
            Ok(())
        } else {
            Err(RepositoryError::InvalidPath(path.to_owned()))
        }
    }
}
