//! External SingleFile CLI adapter, separate from repository persistence.
//!
//! Captures exactly one HTTP(S) URL as an HTML file in a temporary directory.
//! The caller decides whether and how to register the resulting bytes as an
//! Asset; this module never writes into the repository.

use crate::model::HttpUrl;
use std::{
    error::Error,
    ffi::OsString,
    fmt, fs,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
    process::{Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_MAX_BYTES: u64 = 128 * 1024 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(25);
const STDERR_LIMIT: u64 = 4096;

#[derive(Debug)]
pub enum CaptureError {
    Io {
        operation: &'static str,
        source: io::Error,
    },
    Failed {
        status: ExitStatus,
        stderr: String,
    },
    TimedOut(Duration),
    MissingOutput,
    NotRegularFile,
    EmptyOutput,
    TooLarge {
        limit: u64,
    },
    InvalidUtf8,
    NotHtml,
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { operation, source } => write!(f, "SingleFile {operation}: {source}"),
            Self::Failed { status, stderr } => {
                write!(f, "SingleFile failed ({status})")?;
                if !stderr.is_empty() {
                    write!(f, ": {stderr}")?;
                }
                Ok(())
            }
            Self::TimedOut(duration) => write!(f, "SingleFile timed out after {duration:?}"),
            Self::MissingOutput => f.write_str("SingleFile did not create an HTML file"),
            Self::NotRegularFile => f.write_str("SingleFile output is not a regular file"),
            Self::EmptyOutput => f.write_str("SingleFile generated an empty HTML file"),
            Self::TooLarge { limit } => write!(f, "SingleFile HTML exceeds {limit} bytes"),
            Self::InvalidUtf8 => f.write_str("SingleFile HTML is not valid UTF-8"),
            Self::NotHtml => {
                f.write_str("SingleFile output does not appear to be an HTML document")
            }
        }
    }
}

impl Error for CaptureError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn io_error(operation: &'static str, source: io::Error) -> CaptureError {
    CaptureError::Io { operation, source }
}

/// Runs `single-file URL OUTPUT` directly (without a shell).
///
/// An alternate executable can be provided for installation-specific paths
/// or for deterministic integration tests. No extra SingleFile flags are
/// accepted at this layer; they can be introduced deliberately when needed.
#[derive(Clone, Debug)]
pub struct SingleFileCapture {
    executable: OsString,
    timeout: Duration,
    max_bytes: u64,
}

impl Default for SingleFileCapture {
    fn default() -> Self {
        Self::new("single-file")
    }
}

impl SingleFileCapture {
    pub fn new(executable: impl Into<OsString>) -> Self {
        Self {
            executable: executable.into(),
            timeout: DEFAULT_TIMEOUT,
            max_bytes: DEFAULT_MAX_BYTES,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_max_bytes(mut self, max_bytes: u64) -> Self {
        self.max_bytes = max_bytes;
        self
    }

    /// Return verified HTML bytes without persisting an Asset.
    ///
    /// `HttpUrl` is validated at the model boundary. The supplied URL and
    /// output path are passed as separate arguments (never interpolated into
    /// a shell command). Temporary files are removed on success and failure.
    pub fn capture(&self, url: &HttpUrl) -> Result<Vec<u8>, CaptureError> {
        let temporary = tempfile::tempdir().map_err(|e| io_error("create temp dir", e))?;
        let output = temporary.path().join("snapshot.html");
        let stderr_file = tempfile::tempfile().map_err(|e| io_error("create stderr file", e))?;
        let mut child = Command::new(&self.executable)
            .arg(url.as_str())
            .arg(&output)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(
                stderr_file
                    .try_clone()
                    .map_err(|e| io_error("open stderr file", e))?,
            ))
            .spawn()
            .map_err(|e| io_error("launch", e))?;

        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if started.elapsed() >= self.timeout => {
                    // Kill and reap the direct child. Browsers spawned by
                    // SingleFile may require separate cleanup in the future.
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(CaptureError::TimedOut(self.timeout));
                }
                Ok(None) => thread::sleep(POLL_INTERVAL),
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(io_error("wait for process", error));
                }
            }
        };

        if !status.success() {
            let stderr = read_stderr(&stderr_file)?;
            return Err(CaptureError::Failed { status, stderr });
        }
        read_html(&output, self.max_bytes)
    }
}

fn read_stderr(file: &fs::File) -> Result<String, CaptureError> {
    let mut reader = file.try_clone().map_err(|e| io_error("open stderr", e))?;
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|e| io_error("seek stderr", e))?;
    let mut bytes = Vec::new();
    reader
        .take(STDERR_LIMIT)
        .read_to_end(&mut bytes)
        .map_err(|e| io_error("read stderr", e))?;
    Ok(String::from_utf8_lossy(&bytes).trim().to_owned())
}

fn read_html(path: &Path, limit: u64) -> Result<Vec<u8>, CaptureError> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(CaptureError::MissingOutput);
        }
        Err(error) => return Err(io_error("stat HTML", error)),
    };
    if !meta.file_type().is_file() {
        return Err(CaptureError::NotRegularFile);
    }
    if meta.len() > limit {
        return Err(CaptureError::TooLarge { limit });
    }
    let file = fs::File::open(path).map_err(|e| io_error("open HTML", e))?;
    if !file
        .metadata()
        .map_err(|e| io_error("stat opened HTML", e))?
        .file_type()
        .is_file()
    {
        return Err(CaptureError::NotRegularFile);
    }
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| io_error("read HTML", e))?;
    if bytes.len() as u64 > limit {
        return Err(CaptureError::TooLarge { limit });
    }
    if bytes.is_empty() {
        return Err(CaptureError::EmptyOutput);
    }
    let html = std::str::from_utf8(&bytes).map_err(|_| CaptureError::InvalidUtf8)?;
    // SingleFile writes complete HTML documents, not arbitrary text files.
    // Check only the beginning to avoid scanning large snapshots twice.
    let prefix = html
        .chars()
        .take(8192)
        .collect::<String>()
        .to_ascii_lowercase();
    if !prefix.contains("<!doctype html") && !prefix.contains("<html") {
        return Err(CaptureError::NotHtml);
    }
    Ok(bytes)
}
