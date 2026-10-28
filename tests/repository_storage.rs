//! The filesystem layer is intentionally tested without CLI or entity serde.
use digital_notes::{
    model::{AssetId, CardId, WorkId},
    repository::{Repository, RepositoryError},
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "digital-notes-storage-test-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("fresh test directory");
        Self(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove test directory");
    }
}

fn work_id() -> WorkId {
    "work0001".parse().unwrap()
}

#[test]
fn initialization_and_managed_paths() {
    let temp = TestRoot::new();
    let repo = Repository::init(temp.path()).unwrap();
    assert_eq!(repo.root(), temp.path());
    for dir in [".pri", "works", "assets", "cards"] {
        assert!(temp.path().join(dir).is_dir());
    }
    assert!(Repository::open(temp.path()).is_ok());
    assert!(Repository::init(temp.path()).is_ok());
    assert_eq!(
        repo.work_path(&work_id()),
        temp.path().join("works/work0001.json")
    );
    let asset: AssetId = "asset001".parse().unwrap();
    assert_eq!(
        repo.asset_path(&asset),
        temp.path().join("assets/asset001.json")
    );
    assert_eq!(
        repo.html_path(&asset),
        temp.path().join("assets/asset001.html")
    );
    let card: CardId = "card0001".parse().unwrap();
    assert_eq!(
        repo.card_path(&card),
        temp.path().join("cards/card0001.json")
    );
}

#[test]
fn create_read_and_duplicate_protection() {
    let temp = TestRoot::new();
    let repo = Repository::init(temp.path()).unwrap();
    let path = repo.work_path(&work_id());
    let guard = repo.begin_write().unwrap();
    let revision = guard.create(&path, b"first version").unwrap();
    let read = repo.read(&path).unwrap();
    assert_eq!(read.bytes, b"first version");
    assert_eq!(read.revision, revision);
    assert!(matches!(
        guard.create(&path, b"second version"),
        Err(RepositoryError::AlreadyExists(_))
    ));
    assert_eq!(repo.read(&path).unwrap().bytes, b"first version");
    let staged: Vec<_> = fs::read_dir(temp.path().join("works"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".dn-tmp-"))
        .collect();
    assert!(staged.is_empty(), "staging files leaked: {staged:?}");
}

#[test]
fn replacement_checks_revision_and_preserves_old_file_on_conflict() {
    let temp = TestRoot::new();
    let repo = Repository::init(temp.path()).unwrap();
    let path = repo.work_path(&work_id());
    let guard = repo.begin_write().unwrap();
    let original = guard.create(&path, b"one").unwrap();
    let second = guard
        .replace_if_unchanged(&path, &original, b"two")
        .unwrap();
    assert_ne!(original, second);
    assert!(matches!(
        guard.replace_if_unchanged(&path, &original, b"stale"),
        Err(RepositoryError::Conflict(_))
    ));
    assert_eq!(repo.read(&path).unwrap().bytes, b"two");
    assert_eq!(
        guard.replace_if_unchanged(&path, &second, b"two").unwrap(),
        second
    );
    assert_eq!(repo.read(&path).unwrap().revision, second);
}

#[test]
fn writes_and_reads_are_limited_to_managed_file_names() {
    let temp = TestRoot::new();
    let repo = Repository::init(temp.path()).unwrap();
    let outside = temp.path().join("outside.json");
    let wrong_extension = temp.path().join("works/work0001.html");
    let invalid_id = temp.path().join("works/../assets/asset001.json");
    let guard = repo.begin_write().unwrap();
    for path in [&outside, &wrong_extension, &invalid_id] {
        assert!(matches!(
            guard.create(path, b"payload"),
            Err(RepositoryError::InvalidPath(_))
        ));
        assert!(matches!(
            repo.read(path),
            Err(RepositoryError::InvalidPath(_))
        ));
    }
    assert!(!outside.exists());
}

#[test]
fn write_lock_serializes_independent_handles() {
    let temp = TestRoot::new();
    let repo = Repository::init(temp.path()).unwrap();
    let guard = repo.begin_write().unwrap();
    let root = temp.path().to_owned();
    let (started_tx, started_rx) = mpsc::channel();
    let (acquired_tx, acquired_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let other = Repository::open(&root).unwrap();
        started_tx.send(()).unwrap();
        let _second = other.begin_write().unwrap();
        acquired_tx.send(()).unwrap();
    });
    started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(
        acquired_rx
            .recv_timeout(Duration::from_millis(100))
            .is_err()
    );
    drop(guard);
    acquired_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    worker.join().unwrap();
}

#[cfg(unix)]
#[test]
fn symlinks_are_not_accepted_as_managed_files_or_directories() {
    use std::os::unix::fs::symlink;
    let temp = TestRoot::new();
    let repo = Repository::init(temp.path()).unwrap();
    let outside = temp.path().join("outside.json");
    fs::write(&outside, b"outside").unwrap();
    let work_path = repo.work_path(&work_id());
    symlink(&outside, &work_path).unwrap();
    assert!(matches!(
        repo.read(&work_path),
        Err(RepositoryError::InvalidPath(_))
    ));
    assert!(matches!(
        repo.begin_write()
            .unwrap()
            .create(&work_path, b"should not replace"),
        Err(RepositoryError::AlreadyExists(_))
    ));
    assert_eq!(fs::read(&outside).unwrap(), b"outside");

    let other = temp.path().join("replacement");
    fs::create_dir(&other).unwrap();
    fs::remove_dir(temp.path().join("cards")).unwrap();
    symlink(&other, temp.path().join("cards")).unwrap();
    assert!(matches!(
        Repository::open(temp.path()),
        Err(RepositoryError::InvalidLayout(_))
    ));
}

#[cfg(unix)]
#[test]
fn lock_file_cannot_be_a_symlink() {
    use std::os::unix::fs::symlink;
    let temp = TestRoot::new();
    let repo = Repository::init(temp.path()).unwrap();
    let outside = temp.path().join("outside.lock");
    fs::write(&outside, b"unchanged").unwrap();
    symlink(&outside, temp.path().join(".pri/write.lock")).unwrap();
    assert!(matches!(
        repo.begin_write(),
        Err(RepositoryError::InvalidLayout(_))
    ));
    assert_eq!(fs::read(outside).unwrap(), b"unchanged");
}
