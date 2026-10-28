use digital_notes::{
    model::{Title, UtcTimestamp, Work, WorkId, WorkStatus},
    repository::{Repository, RepositoryError},
};
use std::{
    cell::Cell,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let seq = NEXT_TEST.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "digital-notes-work-test-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
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

fn fixture() -> Work {
    serde_json::from_str(include_str!("fixtures/work.json")).unwrap()
}

fn work_id() -> WorkId {
    "work0001".parse().unwrap()
}

#[test]
fn work_create_load_and_duplicate_protection() {
    let temp = TestRoot::new();
    let repo = Repository::init(temp.path()).unwrap();
    let original = fixture();
    let created = repo.create_work(original.clone()).unwrap();
    assert_eq!(created.value, original);
    assert_eq!(repo.load_work(&original.id).unwrap().value, original);
    assert_eq!(
        repo.load_work(&original.id).unwrap().revision,
        created.revision
    );
    assert!(matches!(
        repo.create_work(original),
        Err(RepositoryError::AlreadyExists(_))
    ));
    let file = fs::read_to_string(repo.work_path(&work_id())).unwrap();
    assert!(file.ends_with('\n'));
    assert!(file.contains("\"source_work_ids\""));
    assert_eq!(fs::read_dir(temp.path().join("assets")).unwrap().count(), 0);
}

#[test]
fn update_work_status_body_and_title_sets_updated_and_revision() {
    let temp = TestRoot::new();
    let repo = Repository::init(temp.path()).unwrap();
    let created = repo.create_work(fixture()).unwrap();
    let edited = repo
        .update_work(&created.value.id, &created.revision, |work| {
            work.status = WorkStatus::Done;
            work.body = "## Finished\n".to_owned();
            work.title = "Reviewed papers".parse::<Title>().unwrap();
        })
        .unwrap();
    assert_eq!(edited.value.status, WorkStatus::Done);
    assert_eq!(edited.value.body, "## Finished\n");
    assert_eq!(edited.value.title.as_str(), "Reviewed papers");
    assert_eq!(edited.value.created, created.value.created);
    assert!(edited.value.updated >= created.value.updated);
    assert_ne!(edited.revision, created.revision);
    let loaded = repo.load_work(&created.value.id).unwrap();
    assert_eq!(loaded.value, edited.value);
    assert_eq!(loaded.revision, edited.revision);

    // Both active -> done and done -> active are allowed.
    let active = repo
        .update_work(&created.value.id, &edited.revision, |work| {
            work.status = WorkStatus::Active;
        })
        .unwrap();
    assert_eq!(active.value.status, WorkStatus::Active);
}

#[test]
fn no_op_does_not_touch_file_or_revision() {
    let temp = TestRoot::new();
    let repo = Repository::init(temp.path()).unwrap();
    let created = repo.create_work(fixture()).unwrap();
    let path = repo.work_path(&created.value.id);
    let old_bytes = fs::read(&path).unwrap();
    let unchanged = repo
        .update_work(&created.value.id, &created.revision, |_| {})
        .unwrap();
    assert_eq!(unchanged.revision, created.revision);
    assert_eq!(unchanged.value, created.value);
    assert_eq!(fs::read(path).unwrap(), old_bytes);
}

#[test]
fn stale_revision_rejected_before_mutation() {
    let temp = TestRoot::new();
    let repo = Repository::init(temp.path()).unwrap();
    let created = repo.create_work(fixture()).unwrap();
    let edited = repo
        .update_work(&created.value.id, &created.revision, |work| {
            work.status = WorkStatus::Done;
        })
        .unwrap();
    let called = Cell::new(false);
    assert!(matches!(
        repo.update_work(&created.value.id, &created.revision, |_| {
            called.set(true);
        }),
        Err(RepositoryError::Conflict(_))
    ));
    assert!(!called.get());
    assert_eq!(
        repo.load_work(&created.value.id).unwrap().value,
        edited.value
    );
}

#[test]
fn immutable_fields_are_rejected_without_writing() {
    let temp = TestRoot::new();
    let repo = Repository::init(temp.path()).unwrap();
    let created = repo.create_work(fixture()).unwrap();
    let id = &created.value.id;
    let path = repo.work_path(id);
    let old_bytes = fs::read(&path).unwrap();
    for change in ["id", "created", "updated"] {
        let result = repo.update_work(id, &created.revision, |work| match change {
            "id" => work.id = "work0002".parse().unwrap(),
            "created" => work.created = "2026-10-27T10:00:00Z".parse().unwrap(),
            "updated" => work.updated = "2026-10-29T10:00:00Z".parse().unwrap(),
            _ => unreachable!(),
        });
        assert!(
            matches!(result, Err(RepositoryError::InvalidWork(_))),
            "{change}"
        );
        assert_eq!(fs::read(&path).unwrap(), old_bytes, "{change}");
    }
}

#[test]
fn rejects_invalid_local_work_invariants() {
    let temp = TestRoot::new();
    let repo = Repository::init(temp.path()).unwrap();
    let mut bad = fixture();
    bad.updated = "2026-10-27T10:00:00Z".parse::<UtcTimestamp>().unwrap();
    assert!(matches!(
        repo.create_work(bad),
        Err(RepositoryError::InvalidWork(_))
    ));

    let mut bad = fixture();
    bad.updated = "2026-10-29T10:00:00Z".parse().unwrap();
    assert!(matches!(
        repo.create_work(bad),
        Err(RepositoryError::InvalidWork(_))
    ));

    let mut bad = fixture();
    bad.source_work_ids.push(work_id());
    assert!(matches!(
        repo.create_work(bad),
        Err(RepositoryError::InvalidWork(_))
    ));

    let mut bad = fixture();
    bad.source_work_ids = vec!["work0002".parse().unwrap(); 2];
    assert!(matches!(
        repo.create_work(bad),
        Err(RepositoryError::InvalidWork(_))
    ));
    assert_eq!(fs::read_dir(temp.path().join("works")).unwrap().count(), 0);
}

#[test]
fn invalid_update_does_not_change_persisted_work() {
    let temp = TestRoot::new();
    let repo = Repository::init(temp.path()).unwrap();
    let created = repo.create_work(fixture()).unwrap();
    assert!(matches!(
        repo.update_work(&created.value.id, &created.revision, |work| {
            work.source_work_ids = vec!["work0002".parse().unwrap(); 2];
        }),
        Err(RepositoryError::InvalidWork(_))
    ));
    assert_eq!(
        repo.load_work(&created.value.id).unwrap().value,
        created.value
    );
}

#[test]
fn load_rejects_malformed_json_and_filename_id_mismatch() {
    let temp = TestRoot::new();
    let repo = Repository::init(temp.path()).unwrap();
    let guard = repo.begin_write().unwrap();
    let id: WorkId = "work0002".parse().unwrap();
    guard.create(&repo.work_path(&id), b"not-json").unwrap();
    assert!(matches!(
        repo.load_work(&id),
        Err(RepositoryError::Json { .. })
    ));

    let other_id: WorkId = "work0003".parse().unwrap();
    let json = serde_json::to_vec(&fixture()).unwrap();
    guard.create(&repo.work_path(&other_id), &json).unwrap();
    assert!(matches!(
        repo.load_work(&other_id),
        Err(RepositoryError::InvalidWork(_))
    ));
}

#[test]
fn missing_work_is_not_found() {
    let temp = TestRoot::new();
    let repo = Repository::init(temp.path()).unwrap();
    assert!(matches!(
        repo.load_work(&work_id()),
        Err(RepositoryError::NotFound(_))
    ));
}
