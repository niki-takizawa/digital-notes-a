//! Repository-wide integrity checks are intentionally opt-in until the CLI
//! check command and mutation-time reference policy are finalized.
use digital_notes::{
    model::{Asset, AssetId, Card, CardId, ContentHash, Work, WorkId},
    repository::{IntegrityIssue, IntegrityIssueKind as Kind, Repository},
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "digital-notes-integrity-{}-{nanos}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn id_work(raw: &str) -> WorkId {
    raw.parse().unwrap()
}
fn id_asset(raw: &str) -> AssetId {
    raw.parse().unwrap()
}
fn id_card(raw: &str) -> CardId {
    raw.parse().unwrap()
}

fn work(raw: &str) -> Work {
    let mut value: Work = serde_json::from_str(include_str!("fixtures/work.json")).unwrap();
    value.id = id_work(raw);
    value
}

fn asset(raw: &str, html: &[u8]) -> Asset {
    let mut value: Asset = serde_json::from_str(include_str!("fixtures/asset.json")).unwrap();
    value.id = id_asset(raw);
    value.content_hash = format!("{:x}", Sha256::digest(html))
        .parse::<ContentHash>()
        .unwrap();
    value
}

fn card(raw: &str) -> Card {
    let mut value: Card = serde_json::from_str(include_str!("fixtures/card.json")).unwrap();
    value.id = id_card(raw);
    value
}

fn valid_repo(root: &TestRoot) -> Repository {
    let repo = Repository::init(&root.0).unwrap();
    repo.create_work(work("work0001")).unwrap();
    repo.create_work(work("work0002")).unwrap();
    for raw in ["asset001", "asset002"] {
        repo.create_asset_pair(asset(raw, b"<p>original</p>"), b"<p>original</p>")
            .unwrap();
    }
    repo.create_card(card("card0001")).unwrap();
    let mut second = card("card0002");
    second.source_card_ids.push(id_card("card0001"));
    repo.create_card(second).unwrap();
    repo
}

fn includes(issues: &[IntegrityIssue], kind: Kind, path: &Path, contains: &str) -> bool {
    issues.iter().any(|issue| {
        issue.kind == kind && issue.path.as_path() == path && issue.detail.contains(contains)
    })
}

#[test]
fn valid_graph_has_no_issues_and_is_deterministic() {
    let root = TestRoot::new();
    let repo = valid_repo(&root);
    assert!(repo.check_references().unwrap().is_empty());
    assert_eq!(
        repo.check_references().unwrap(),
        repo.check_references().unwrap()
    );
}

#[test]
fn work_and_card_reference_gaps_are_reported_with_origin() {
    let root = TestRoot::new();
    let repo = valid_repo(&root);
    let original = repo.load_work(&id_work("work0001")).unwrap();
    repo.update_work(&original.value.id, &original.revision, |w| {
        w.source_work_ids.push(id_work("work9999"));
    })
    .unwrap();
    let loaded = repo.load_card(&id_card("card0001")).unwrap();
    repo.update_card(&loaded.value.id, &loaded.revision, |c| {
        c.source_work_ids.push(id_work("work9999"));
        c.source_asset_ids.push(id_asset("asset999"));
        c.source_card_ids.push(id_card("card9999"));
    })
    .unwrap();
    let issues = repo.check_references().unwrap();
    assert_eq!(issues.len(), 4, "{issues:?}");
    assert!(includes(
        &issues,
        Kind::MissingWork,
        &repo.work_path(&id_work("work0001")),
        "work9999"
    ));
    assert!(includes(
        &issues,
        Kind::MissingWork,
        &repo.card_path(&id_card("card0001")),
        "work9999"
    ));
    assert!(includes(
        &issues,
        Kind::MissingAsset,
        &repo.card_path(&id_card("card0001")),
        "asset999"
    ));
    assert!(includes(
        &issues,
        Kind::MissingCard,
        &repo.card_path(&id_card("card0001")),
        "card9999"
    ));
}

#[test]
fn deleted_work_is_reported_for_asset_and_card() {
    let root = TestRoot::new();
    let repo = valid_repo(&root);
    fs::remove_file(repo.work_path(&id_work("work0002"))).unwrap();
    let issues = repo.check_references().unwrap();
    assert_eq!(
        issues
            .iter()
            .filter(|issue| issue.kind == Kind::MissingWork)
            .count(),
        4
    );
    assert!(includes(
        &issues,
        Kind::MissingWork,
        &repo.asset_path(&id_asset("asset001")),
        "related_work_id: work0002"
    ));
    assert!(includes(
        &issues,
        Kind::MissingWork,
        &repo.card_path(&id_card("card0001")),
        "source_work_id: work0002"
    ));
}

#[test]
fn deleted_creator_work_is_detected_in_assets_and_cards() {
    let root = TestRoot::new();
    let repo = valid_repo(&root);
    fs::remove_file(repo.work_path(&id_work("work0001"))).unwrap();
    let issues = repo.check_references().unwrap();
    assert_eq!(
        issues
            .iter()
            .filter(|issue| issue.kind == Kind::MissingWork)
            .count(),
        4
    );
    assert!(includes(
        &issues,
        Kind::MissingWork,
        &repo.asset_path(&id_asset("asset001")),
        "created_by_work_id: work0001"
    ));
    assert!(includes(
        &issues,
        Kind::MissingWork,
        &repo.card_path(&id_card("card0002")),
        "created_by_work_id: work0001"
    ));
}

#[test]
fn orphan_html_and_missing_html_are_separately_reported() {
    let root = TestRoot::new();
    let repo = valid_repo(&root);
    fs::write(repo.html_path(&id_asset("asset999")), b"HTML only").unwrap();
    fs::remove_file(repo.html_path(&id_asset("asset002"))).unwrap();
    let issues = repo.check_references().unwrap();
    assert!(includes(
        &issues,
        Kind::OrphanAssetHtml,
        &repo.html_path(&id_asset("asset999")),
        "asset999"
    ));
    assert!(includes(
        &issues,
        Kind::MissingAssetHtml,
        &repo.asset_path(&id_asset("asset002")),
        "asset002"
    ));
    // A JSON sidecar with missing HTML is not a missing Asset reference.
    assert!(!issues.iter().any(|issue| issue.kind == Kind::MissingAsset));
}

#[test]
fn corruption_is_reported_without_double_counting_as_missing() {
    let root = TestRoot::new();
    let repo = valid_repo(&root);
    fs::write(repo.html_path(&id_asset("asset001")), b"changed").unwrap();
    fs::write(repo.card_path(&id_card("card0001")), b"not json").unwrap();
    let issues = repo.check_references().unwrap();
    assert!(includes(
        &issues,
        Kind::InvalidEntity,
        &repo.asset_path(&id_asset("asset001")),
        "SHA-256"
    ));
    assert!(includes(
        &issues,
        Kind::InvalidEntity,
        &repo.card_path(&id_card("card0001")),
        "invalid JSON"
    ));
    assert!(
        !issues
            .iter()
            .any(|issue| issue.kind == Kind::MissingAsset || issue.kind == Kind::MissingCard)
    );
}

#[test]
fn unexpected_files_and_directories_are_rejected() {
    let root = TestRoot::new();
    let repo = valid_repo(&root);
    let strange = root.0.join("works").join("file.tmp");
    fs::write(&strange, b"ignored").unwrap();
    let directory = root.0.join("cards").join("card9999.json");
    fs::create_dir(&directory).unwrap();
    let issues = repo.check_references().unwrap();
    assert!(includes(
        &issues,
        Kind::InvalidEntry,
        &strange,
        "Work file name"
    ));
    assert!(includes(
        &issues,
        Kind::InvalidEntry,
        &directory,
        "regular file"
    ));
}

#[cfg(unix)]
#[test]
fn symlink_is_rejected_without_following_it() {
    use std::os::unix::fs::symlink;
    let root = TestRoot::new();
    let repo = valid_repo(&root);
    let path = root.0.join("works").join("work9999.json");
    symlink(repo.work_path(&id_work("work0001")), &path).unwrap();
    let issues = repo.check_references().unwrap();
    assert!(includes(
        &issues,
        Kind::InvalidEntry,
        &path,
        "symlinks are forbidden"
    ));
}
