//! Repository-level tests for immutable HTML + mutable Asset JSON sidecars.
use digital_notes::{
    model::{Asset, AssetId, ContentHash, Title, UtcTimestamp, Work, WorkId},
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
const HTML: &[u8] = b"<!doctype html><html><body>Research snapshot</body></html>";
// Computed independently as SHA-256(HTML); not derived from the code under test.
const HTML_SHA256: &str = "11236d1d813f3348d0cfb3a95835e7a8ab01c8c10dc88a947264d10c724c467c";

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = NEXT_TEST.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "digital-notes-asset-test-{}-{nanos}-{seq}",
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

fn work(id: &str) -> Work {
    let mut work: Work = serde_json::from_str(include_str!("fixtures/work.json")).unwrap();
    work.id = id.parse().unwrap();
    work
}

fn asset() -> Asset {
    let mut asset: Asset = serde_json::from_str(include_str!("fixtures/asset.json")).unwrap();
    asset.related_work_ids.clear();
    asset.content_hash = HTML_SHA256.parse().unwrap();
    asset
}

fn repo_with_creator(temp: &TestRoot) -> Repository {
    let repo = Repository::init(temp.path()).unwrap();
    repo.create_work(work("work0001")).unwrap();
    repo
}

fn asset_id() -> AssetId {
    "asset001".parse().unwrap()
}

fn another_work_id() -> WorkId {
    "work0002".parse().unwrap()
}

#[test]
fn create_and_read_asset_pair_and_duplicate_rejection() {
    let temp = TestRoot::new();
    let repo = repo_with_creator(&temp);
    let expected = asset();
    let created = repo.create_asset_pair(expected.clone(), HTML).unwrap();
    assert_eq!(created.value, expected);
    assert_eq!(repo.load_asset(&expected.id).unwrap().value, expected);
    assert_eq!(repo.load_asset_html(&expected.id).unwrap().as_slice(), HTML);
    assert_eq!(
        repo.load_asset(&expected.id).unwrap().revision,
        created.revision
    );
    assert_eq!(
        fs::read(repo.html_path(&expected.id)).unwrap().as_slice(),
        HTML
    );
    assert!(
        fs::read_to_string(repo.asset_path(&expected.id))
            .unwrap()
            .ends_with('\n')
    );
    assert!(matches!(
        repo.create_asset_pair(expected, HTML),
        Err(RepositoryError::AlreadyExists(_))
    ));
    assert_eq!(
        fs::read(repo.html_path(&asset_id())).unwrap().as_slice(),
        HTML
    );
}

#[test]
fn creator_work_must_exist_before_asset_capture() {
    let temp = TestRoot::new();
    let repo = Repository::init(temp.path()).unwrap();
    let a = asset();
    assert!(matches!(
        repo.create_asset_pair(a.clone(), HTML),
        Err(RepositoryError::NotFound(_))
    ));
    assert!(!repo.html_path(&a.id).exists());
    assert!(!repo.asset_path(&a.id).exists());

    repo.create_work(work("work0001")).unwrap();
    assert!(repo.create_asset_pair(a, HTML).is_ok());
}

#[test]
fn rejects_bad_hash_and_empty_html_without_writing() {
    let temp = TestRoot::new();
    let repo = repo_with_creator(&temp);
    let a = asset();
    assert!(matches!(
        repo.create_asset_pair(a.clone(), b"wrong"),
        Err(RepositoryError::InvalidAsset(_))
    ));
    assert!(matches!(
        repo.create_asset_pair(a.clone(), b""),
        Err(RepositoryError::InvalidAsset(_))
    ));
    assert!(!repo.html_path(&a.id).exists());
    assert!(!repo.asset_path(&a.id).exists());
}

#[test]
fn rejects_duplicate_creator_in_related_and_invalid_dates() {
    let temp = TestRoot::new();
    let repo = repo_with_creator(&temp);
    let mut bad = asset();
    bad.related_work_ids = vec!["work0001".parse().unwrap()];
    assert!(matches!(
        repo.create_asset_pair(bad, HTML),
        Err(RepositoryError::InvalidAsset(_))
    ));
    let mut bad = asset();
    bad.related_work_ids = vec![another_work_id(), another_work_id()];
    assert!(matches!(
        repo.create_asset_pair(bad, HTML),
        Err(RepositoryError::InvalidAsset(_))
    ));
    let mut bad = asset();
    bad.updated = "2026-10-27T10:00:00Z".parse::<UtcTimestamp>().unwrap();
    assert!(matches!(
        repo.create_asset_pair(bad, HTML),
        Err(RepositoryError::InvalidAsset(_))
    ));
    let mut bad = asset();
    bad.updated = "2026-10-29T10:00:00Z".parse().unwrap();
    assert!(matches!(
        repo.create_asset_pair(bad, HTML),
        Err(RepositoryError::InvalidAsset(_))
    ));
    assert_eq!(fs::read_dir(temp.path().join("assets")).unwrap().count(), 0);
}

#[test]
fn rejects_unknown_related_work_on_create_or_update() {
    let temp = TestRoot::new();
    let repo = repo_with_creator(&temp);
    let mut a = asset();
    a.related_work_ids = vec![another_work_id()];
    assert!(matches!(
        repo.create_asset_pair(a, HTML),
        Err(RepositoryError::NotFound(_))
    ));
    let created = repo.create_asset_pair(asset(), HTML).unwrap();
    assert!(matches!(
        repo.update_asset(&created.value.id, &created.revision, |a| {
            a.related_work_ids.push(another_work_id());
        }),
        Err(RepositoryError::NotFound(_))
    ));
    assert_eq!(repo.load_asset(&asset_id()).unwrap().value, created.value);
}

#[test]
fn update_metadata_and_related_work_preserves_html() {
    let temp = TestRoot::new();
    let repo = repo_with_creator(&temp);
    repo.create_work(work("work0002")).unwrap();
    let created = repo.create_asset_pair(asset(), HTML).unwrap();
    let edited = repo
        .update_asset(&asset_id(), &created.revision, |a| {
            a.title = "Revised title".parse::<Title>().unwrap();
            a.asset_type = "paper".parse().unwrap();
            a.related_work_ids.push(another_work_id());
        })
        .unwrap();
    assert_eq!(edited.value.title.as_str(), "Revised title");
    assert_eq!(edited.value.related_work_ids, vec![another_work_id()]);
    assert_eq!(edited.value.created, created.value.created);
    assert!(edited.value.updated >= created.value.updated);
    assert_ne!(edited.revision, created.revision);
    assert_eq!(repo.load_asset(&asset_id()).unwrap().value, edited.value);
    assert_eq!(
        fs::read(repo.html_path(&asset_id())).unwrap().as_slice(),
        HTML
    );
}

#[test]
fn no_op_preserves_exact_json_and_revision() {
    let temp = TestRoot::new();
    let repo = repo_with_creator(&temp);
    let created = repo.create_asset_pair(asset(), HTML).unwrap();
    let original = fs::read(repo.asset_path(&asset_id())).unwrap();
    let result = repo
        .update_asset(&asset_id(), &created.revision, |_| {})
        .unwrap();
    assert_eq!(created.revision, result.revision);
    assert_eq!(created.value, result.value);
    assert_eq!(fs::read(repo.asset_path(&asset_id())).unwrap(), original);
}

#[test]
fn stale_revision_prevents_mutator_execution() {
    let temp = TestRoot::new();
    let repo = repo_with_creator(&temp);
    let created = repo.create_asset_pair(asset(), HTML).unwrap();
    let updated = repo
        .update_asset(&asset_id(), &created.revision, |a| {
            a.title = "Updated".parse().unwrap();
        })
        .unwrap();
    let called = Cell::new(false);
    assert!(matches!(
        repo.update_asset(&asset_id(), &created.revision, |_| {
            called.set(true);
        }),
        Err(RepositoryError::Conflict(_))
    ));
    assert!(!called.get());
    assert_eq!(repo.load_asset(&asset_id()).unwrap().value, updated.value);
}

#[test]
fn immutable_metadata_fields_are_protected() {
    let temp = TestRoot::new();
    let repo = repo_with_creator(&temp);
    let created = repo.create_asset_pair(asset(), HTML).unwrap();
    let original = fs::read(repo.asset_path(&asset_id())).unwrap();
    for changed in [
        "created_by",
        "hash",
        "url",
        "capture_method",
        "updated",
        "created",
        "id",
    ] {
        let result = repo.update_asset(&asset_id(), &created.revision, |a| match changed {
            "created_by" => a.created_by_work_id = another_work_id(),
            "hash" => a.content_hash = "0".repeat(64).parse::<ContentHash>().unwrap(),
            "url" => a.url = "https://example.com/changed".parse().unwrap(),
            "capture_method" => a.capture_method = "other".parse().unwrap(),
            "updated" => a.updated = "2026-10-29T11:00:00Z".parse().unwrap(),
            "created" => a.created = "2026-10-27T11:00:00Z".parse().unwrap(),
            "id" => a.id = "asset002".parse().unwrap(),
            _ => unreachable!(),
        });
        assert!(
            matches!(result, Err(RepositoryError::InvalidAsset(_))),
            "{changed}"
        );
        assert_eq!(fs::read(repo.asset_path(&asset_id())).unwrap(), original);
    }
}

#[test]
fn load_rejects_corrupted_html_and_missing_html() {
    let temp = TestRoot::new();
    let repo = repo_with_creator(&temp);
    let created = repo.create_asset_pair(asset(), HTML).unwrap();
    let html_path = repo.html_path(&asset_id());
    fs::write(&html_path, b"modified outside repository").unwrap();
    assert!(matches!(
        repo.load_asset(&asset_id()),
        Err(RepositoryError::InvalidAsset(_))
    ));
    assert!(matches!(
        repo.update_asset(&asset_id(), &created.revision, |_| {}),
        Err(RepositoryError::InvalidAsset(_))
    ));
    fs::remove_file(html_path).unwrap();
    assert!(matches!(
        repo.load_asset(&asset_id()),
        Err(RepositoryError::NotFound(_))
    ));
}

#[test]
fn load_rejects_malformed_json_and_filename_mismatch() {
    let temp = TestRoot::new();
    let repo = repo_with_creator(&temp);
    let guard = repo.begin_write().unwrap();
    let id: AssetId = "asset002".parse().unwrap();
    guard
        .create(&repo.asset_path(&id), b"invalid-json")
        .unwrap();
    assert!(matches!(
        repo.load_asset(&id),
        Err(RepositoryError::Json { .. })
    ));
    let other: AssetId = "asset003".parse().unwrap();
    let json = serde_json::to_vec(&asset()).unwrap();
    guard.create(&repo.asset_path(&other), &json).unwrap();
    assert!(matches!(
        repo.load_asset(&other),
        Err(RepositoryError::InvalidAsset(_))
    ));
}

#[test]
fn existing_or_orphaned_path_cannot_be_overwritten() {
    let temp = TestRoot::new();
    let repo = repo_with_creator(&temp);
    let a = asset();
    let html_path = repo.html_path(&a.id);
    fs::write(&html_path, b"orphan").unwrap();
    assert!(matches!(
        repo.create_asset_pair(a.clone(), HTML),
        Err(RepositoryError::AlreadyExists(_))
    ));
    assert_eq!(fs::read(&html_path).unwrap(), b"orphan");
    fs::remove_file(html_path).unwrap();
    let json_path = repo.asset_path(&a.id);
    fs::write(&json_path, b"already here").unwrap();
    assert!(matches!(
        repo.create_asset_pair(a, HTML),
        Err(RepositoryError::AlreadyExists(_))
    ));
    assert!(!repo.html_path(&asset_id()).exists());
    assert_eq!(fs::read(&json_path).unwrap(), b"already here");
}

#[cfg(unix)]
#[test]
fn symlinked_html_is_rejected() {
    use std::os::unix::fs::symlink;
    let temp = TestRoot::new();
    let repo = repo_with_creator(&temp);
    let a = asset();
    let outside = temp.path().join("outside.html");
    fs::write(&outside, b"unmodified").unwrap();
    symlink(&outside, repo.html_path(&a.id)).unwrap();
    assert!(matches!(
        repo.create_asset_pair(a, HTML),
        Err(RepositoryError::AlreadyExists(_))
    ));
    assert_eq!(fs::read(outside).unwrap(), b"unmodified");
}
