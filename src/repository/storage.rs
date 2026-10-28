//! Local file storage: serialized writers, complete-file publication and CAS.
//!
//! Staging is performed in the destination directory. This prevents torn JSON
//! reads on ordinary process crashes, but is not a multi-file transaction or a
//! guarantee of recovery after sudden hardware/power failure.

use super::{Repository, RepositoryError, Result, io_at};
use std::{
    fmt, fs,
    fs::{File, OpenOptions},
    io::{self, ErrorKind, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// An exact snapshot, so revision checks are collision-free without a hash
/// dependency. It is deliberately opaque; the representation can change later.
#[derive(Clone, Eq, PartialEq)]
pub struct Revision(Box<[u8]>);

impl fmt::Debug for Revision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Revision(..)")
    }
}

impl Revision {
    fn of(bytes: &[u8]) -> Self {
        Self(bytes.into())
    }

    fn matches(&self, bytes: &[u8]) -> bool {
        self.0.as_ref() == bytes
    }
}

#[derive(Debug)]
pub struct LoadedBytes {
    pub bytes: Vec<u8>,
    pub revision: Revision,
}

pub(super) fn read(path: &Path) -> Result<LoadedBytes> {
    let metadata = fs::symlink_metadata(path).map_err(|err| io_at(path, err))?;
    if !metadata.file_type().is_file() {
        return Err(RepositoryError::InvalidPath(path.to_owned()));
    }
    let mut file = File::open(path).map_err(|err| io_at(path, err))?;
    // Double-check the opened descriptor in case the entry changed during open.
    if !file
        .metadata()
        .map_err(|err| io_at(path, err))?
        .file_type()
        .is_file()
    {
        return Err(RepositoryError::InvalidPath(path.to_owned()));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|err| io_at(path, err))?;
    Ok(LoadedBytes {
        revision: Revision::of(&bytes),
        bytes,
    })
}

/// Owned exclusive lock tied to a particular repository.
/// All public write primitives require this guard.
pub struct WriteGuard<'a> {
    repo: &'a Repository,
    _file: File,
}

pub(super) fn begin_write(repo: &Repository) -> Result<WriteGuard<'_>> {
    let path = repo.root().join(".pri/write.lock");
    match fs::symlink_metadata(&path) {
        Ok(meta) if !meta.file_type().is_file() => {
            return Err(RepositoryError::InvalidLayout(path));
        }
        Ok(_) => {}
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => return Err(io_at(&path, err)),
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|err| io_at(&path, err))?;
    if !file
        .metadata()
        .map_err(|err| io_at(&path, err))?
        .file_type()
        .is_file()
    {
        return Err(RepositoryError::InvalidLayout(path));
    }
    file.lock().map_err(|err| io_at(&path, err))?;
    Ok(WriteGuard { repo, _file: file })
}

/// A temp file is removed if writing or publishing fails.
struct StagingFile(PathBuf);

impl Drop for StagingFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn stage(path: &Path, bytes: &[u8]) -> Result<StagingFile> {
    let parent = path
        .parent()
        .ok_or_else(|| RepositoryError::InvalidPath(path.to_owned()))?;
    for _ in 0..128 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temp = parent.join(format!(".dn-tmp-{}-{sequence}", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(mut file) => {
                let staged = StagingFile(temp);
                file.write_all(bytes)
                    .and_then(|()| file.sync_all())
                    .map_err(|err| io_at(&staged.0, err))?;
                return Ok(staged);
            }
            Err(err) if err.kind() == ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(io_at(&temp, err)),
        }
    }
    Err(RepositoryError::Io {
        path: parent.to_owned(),
        source: io::Error::new(
            ErrorKind::AlreadyExists,
            "unable to allocate a staging file",
        ),
    })
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> Result<()> {
    let parent = path.parent().expect("a managed path has a parent");
    File::open(parent)
        .and_then(|file| file.sync_all())
        .map_err(|err| io_at(parent, err))
}

#[cfg(not(unix))]
fn sync_parent(_path: &Path) -> Result<()> {
    Ok(())
}

impl WriteGuard<'_> {
    /// Publish a new file without overwriting any existing entry, even when
    /// another writer is not cooperating with the advisory lock.
    pub fn create(&self, path: &Path, bytes: &[u8]) -> Result<Revision> {
        self.repo.validate_path(path)?;
        let staged = stage(path, bytes)?;
        fs::hard_link(&staged.0, path).map_err(|err| {
            if err.kind() == ErrorKind::AlreadyExists {
                RepositoryError::AlreadyExists(path.to_owned())
            } else {
                io_at(path, err)
            }
        })?;
        sync_parent(path)?;
        Ok(Revision::of(bytes))
    }

    /// Replace a file only when its on-disk bytes exactly match `expected`.
    /// The lock must cover all higher-level validation in the caller.
    pub fn replace_if_unchanged(
        &self,
        path: &Path,
        expected: &Revision,
        bytes: &[u8],
    ) -> Result<Revision> {
        self.repo.validate_path(path)?;
        let current = read(path)?;
        if !expected.matches(&current.bytes) {
            return Err(RepositoryError::Conflict(path.to_owned()));
        }
        if current.bytes == bytes {
            return Ok(current.revision);
        }
        let staged = stage(path, bytes)?;
        // The caller holds the advisory lock. Rename is atomic for this local
        // single-file replacement on Linux; no partial file is published.
        fs::rename(&staged.0, path).map_err(|err| io_at(path, err))?;
        sync_parent(path)?;
        Ok(Revision::of(bytes))
    }
}
