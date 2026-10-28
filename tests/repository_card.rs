//! Card storage tests: creation flow, evidence metadata, revisions and corruption.
use digital_notes::{
    model::{Card, CardId, Title, UtcTimestamp, Work, WorkId},
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
            .unwrap()
            .as_nanos();
        let seq = NEXT_TEST.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "digital-notes-card-test-{}-{nanos}-{seq}",
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
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn card() -> Card {
    serde_json::from_str(include_str!("fixtures/card.json")).unwrap()
}

fn work(id: &str) -> Work {
    let mut w: Work = serde_json::from_str(include_str!("fixtures/work.json")).unwrap();
    w.id = id.parse().unwrap();
    w
}

fn repo_with_creator(root: &TestRoot) -> Repository {
    let repo = Repository::init(root.path()).unwrap();
    repo.create_work(work("work0001")).unwrap();
    repo
}

fn card_id() -> CardId {
    "card0001".parse().unwrap()
}

fn other_work_id() -> WorkId {
    "work0002".parse().unwrap()
}

#[test]
fn create_load_preserves_multi_asset_references_and_evidence() {
    let root = TestRoot::new();
    let repo = repo_with_creator(&root);
    let expected = card();
    let created = repo.create_card(expected.clone()).unwrap();
    assert_eq!(created.value, expected);
    assert_eq!(created.value.source_asset_ids.len(), 2);
    assert_eq!(created.value.evidence.len(), 1);
    let loaded = repo.load_card(&card_id()).unwrap();
    assert_eq!(loaded.value, expected);
    assert_eq!(loaded.revision, created.revision);
    let json = fs::read_to_string(repo.card_path(&card_id())).unwrap();
    assert!(json.ends_with('\n'));
    assert!(json.contains("\"source_asset_ids\""));
    assert!(matches!(
        repo.create_card(card()),
        Err(RepositoryError::AlreadyExists(_))
    ));
}

#[test]
fn creator_work_must_exist_before_card_creation() {
    let root = TestRoot::new();
    let repo = Repository::init(root.path()).unwrap();
    assert!(matches!(
        repo.create_card(card()),
        Err(RepositoryError::NotFound(_))
    ));
    assert!(!repo.card_path(&card_id()).exists());
    repo.create_work(work("work0001")).unwrap();
    assert!(repo.create_card(card()).is_ok());
}

#[test]
fn new_card_requires_matching_created_and_updated() {
    let root = TestRoot::new();
    let repo = repo_with_creator(&root);
    for date in ["2026-10-27T10:00:00Z", "2026-10-29T10:00:00Z"] {
        let mut bad = card();
        bad.updated = date.parse::<UtcTimestamp>().unwrap();
        assert!(matches!(
            repo.create_card(bad),
            Err(RepositoryError::InvalidCard(_))
        ));
    }
    assert!(!repo.card_path(&card_id()).exists());
}

#[test]
fn rejects_invalid_source_references() {
    let root = TestRoot::new();
    let repo = repo_with_creator(&root);
    let mut cases = Vec::new();
    let mut bad = card();
    bad.source_work_ids.push(other_work_id());
    cases.push(bad);
    let mut bad = card();
    bad.source_work_ids.push("work0001".parse().unwrap());
    cases.push(bad);
    let mut bad = card();
    bad.source_asset_ids.push("asset001".parse().unwrap());
    cases.push(bad);
    let mut bad = card();
    bad.source_card_ids.push(card_id());
    cases.push(bad);
    let mut bad = card();
    bad.source_card_ids = vec!["card0002".parse().unwrap(); 2];
    cases.push(bad);
    for bad in cases {
        assert!(matches!(
            repo.create_card(bad),
            Err(RepositoryError::InvalidCard(_))
        ));
    }
    assert!(!repo.card_path(&card_id()).exists());
}

#[test]
fn rejects_invalid_local_evidence() {
    let root = TestRoot::new();
    let repo = repo_with_creator(&root);
    let mut cases = Vec::new();
    let mut bad = card();
    let repeated = bad.evidence[0].clone();
    bad.evidence.push(repeated);
    cases.push(bad);
    let mut bad = card();
    bad.evidence[0].asset_id = "asset003".parse().unwrap();
    cases.push(bad);
    let mut bad = card();
    bad.evidence[0].selector.exact.clear();
    cases.push(bad);
    let mut bad = card();
    bad.evidence[0].position_selector.end = bad.evidence[0].position_selector.start;
    cases.push(bad);
    let mut bad = card();
    bad.evidence[0].position_selector.end = 0;
    cases.push(bad);
    for bad in cases {
        assert!(matches!(
            repo.create_card(bad),
            Err(RepositoryError::InvalidCard(_))
        ));
    }
    assert!(!repo.card_path(&card_id()).exists());
}

#[test]
fn update_title_body_type_references_and_evidence() {
    let root = TestRoot::new();
    let repo = repo_with_creator(&root);
    let created = repo.create_card(card()).unwrap();
    let edited = repo
        .update_card(&card_id(), &created.revision, |c| {
            c.title = "Updated notes".parse::<Title>().unwrap();
            c.body = "Revised Markdown".to_owned();
            c.card_type = "analysis".parse().unwrap();
            c.source_work_ids.push("work0003".parse().unwrap());
            c.source_asset_ids.push("asset003".parse().unwrap());
            c.source_card_ids.push("card0002".parse().unwrap());
            c.evidence[0].selector.prefix = "Modified prefix".to_owned();
        })
        .unwrap();
    assert_eq!(edited.value.title.as_str(), "Updated notes");
    assert_eq!(edited.value.body, "Revised Markdown");
    assert_eq!(edited.value.card_type.as_str(), "analysis");
    assert_eq!(edited.value.source_asset_ids.len(), 3);
    assert_eq!(edited.value.source_card_ids.len(), 1);
    assert_eq!(edited.value.evidence[0].selector.prefix, "Modified prefix");
    assert_eq!(edited.value.created, created.value.created);
    assert!(edited.value.updated >= created.value.updated);
    assert_ne!(edited.revision, created.revision);
    assert_eq!(repo.load_card(&card_id()).unwrap().value, edited.value);
}

#[test]
fn noop_keeps_json_and_revision() {
    let root = TestRoot::new();
    let repo = repo_with_creator(&root);
    let created = repo.create_card(card()).unwrap();
    let original = fs::read(repo.card_path(&card_id())).unwrap();
    let unchanged = repo
        .update_card(&card_id(), &created.revision, |_| {})
        .unwrap();
    assert_eq!(unchanged.value, created.value);
    assert_eq!(unchanged.revision, created.revision);
    assert_eq!(fs::read(repo.card_path(&card_id())).unwrap(), original);
}

#[test]
fn stale_revision_rejected_before_mutation() {
    let root = TestRoot::new();
    let repo = repo_with_creator(&root);
    let created = repo.create_card(card()).unwrap();
    let edited = repo
        .update_card(&card_id(), &created.revision, |c| {
            c.body = "new body".to_owned();
        })
        .unwrap();
    let called = Cell::new(false);
    assert!(matches!(
        repo.update_card(&card_id(), &created.revision, |_| {
            called.set(true);
        }),
        Err(RepositoryError::Conflict(_))
    ));
    assert!(!called.get());
    assert_eq!(repo.load_card(&card_id()).unwrap().value, edited.value);
}

#[test]
fn immutable_fields_rejected_without_writing() {
    let root = TestRoot::new();
    let repo = repo_with_creator(&root);
    let created = repo.create_card(card()).unwrap();
    let original = fs::read(repo.card_path(&card_id())).unwrap();
    for field in ["id", "created", "updated", "creator"] {
        let result = repo.update_card(&card_id(), &created.revision, |c| match field {
            "id" => c.id = "card0002".parse().unwrap(),
            "created" => c.created = "2026-10-27T10:00:00Z".parse().unwrap(),
            "updated" => c.updated = "2026-10-29T10:00:00Z".parse().unwrap(),
            "creator" => c.created_by_work_id = other_work_id(),
            _ => unreachable!(),
        });
        assert!(
            matches!(result, Err(RepositoryError::InvalidCard(_))),
            "{field}"
        );
        assert_eq!(fs::read(repo.card_path(&card_id())).unwrap(), original);
    }
}

#[test]
fn invalid_update_does_not_change_persisted_card() {
    let root = TestRoot::new();
    let repo = repo_with_creator(&root);
    let created = repo.create_card(card()).unwrap();
    assert!(matches!(
        repo.update_card(&card_id(), &created.revision, |c| {
            c.source_asset_ids.clear();
        }),
        Err(RepositoryError::InvalidCard(_))
    ));
    assert_eq!(repo.load_card(&card_id()).unwrap().value, created.value);
}

#[test]
fn load_rejects_malformed_json_and_filename_mismatch() {
    let root = TestRoot::new();
    let repo = repo_with_creator(&root);
    let guard = repo.begin_write().unwrap();
    let id: CardId = "card0002".parse().unwrap();
    guard
        .create(&repo.card_path(&id), b"not valid json")
        .unwrap();
    assert!(matches!(
        repo.load_card(&id),
        Err(RepositoryError::Json { .. })
    ));
    let other: CardId = "card0003".parse().unwrap();
    let json = serde_json::to_vec(&card()).unwrap();
    guard.create(&repo.card_path(&other), &json).unwrap();
    assert!(matches!(
        repo.load_card(&other),
        Err(RepositoryError::InvalidCard(_))
    ));
}

#[test]
fn load_rejects_invalid_local_invariants() {
    let root = TestRoot::new();
    let repo = repo_with_creator(&root);
    let guard = repo.begin_write().unwrap();
    let mut bad = card();
    bad.evidence[0].position_selector.start = bad.evidence[0].position_selector.end;
    guard
        .create(
            &repo.card_path(&card_id()),
            &serde_json::to_vec(&bad).unwrap(),
        )
        .unwrap();
    assert!(matches!(
        repo.load_card(&card_id()),
        Err(RepositoryError::InvalidCard(_))
    ));
}

#[test]
fn missing_card_reports_not_found() {
    let root = TestRoot::new();
    let repo = repo_with_creator(&root);
    assert!(matches!(
        repo.load_card(&card_id()),
        Err(RepositoryError::NotFound(_))
    ));
}
