//! Evidence registration integration tests: no network or browser required.

use digital_notes::{
    evidence::Resolution,
    model::{
        Asset, AssetEntityType, AssetId, CardId, ContentHash, HashAlgorithm, HtmlFileFormat,
        HttpUrl, SchemaVersion, SnakeCase, Title, UtcTimestamp, WorkId,
    },
    repository::{Repository, RepositoryError},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_digital-notes");

struct Harness {
    temp: TempDir,
}

impl Harness {
    fn new() -> Self {
        let harness = Self {
            temp: tempfile::tempdir().unwrap(),
        };
        harness.ok(&["init"]);
        harness
    }
    fn root(&self) -> &Path {
        self.temp.path()
    }
    fn repo(&self) -> Repository {
        Repository::open(self.root()).unwrap()
    }
    fn invoke(&self, args: &[&str]) -> Output {
        Command::new(BIN)
            .arg("--root")
            .arg(self.root())
            .args(args)
            .env_remove("DIGITAL_NOTES_ROOT")
            .output()
            .unwrap()
    }
    fn ok(&self, args: &[&str]) -> String {
        let result = self.invoke(args);
        assert!(
            result.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        String::from_utf8(result.stdout).unwrap()
    }
    fn fail(&self, args: &[&str]) -> String {
        let result = self.invoke(args);
        assert!(!result.status.success(), "unexpected success for {args:?}");
        String::from_utf8(result.stderr).unwrap()
    }
    fn work(&self) -> String {
        self.ok(&["work", "new", "--title", "Source research", "--id-only"])
            .trim()
            .to_owned()
    }
    fn card(&self, work: &str) -> String {
        self.ok(&[
            "card",
            "new",
            "--title",
            "Analysis",
            "--work",
            work,
            "--id-only",
        ])
        .trim()
        .to_owned()
    }
    fn show(&self, card: &str) -> Value {
        serde_json::from_str(&self.ok(&["card", "show", card, "--json"])).unwrap()
    }
    fn asset(&self, work: &str, html: &[u8]) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let now = UtcTimestamp::now();
        self.repo()
            .create_asset_pair(
                Asset {
                    schema_version: SchemaVersion,
                    id: id.parse::<AssetId>().unwrap(),
                    entity_type: AssetEntityType::Asset,
                    title: "Primary source".parse::<Title>().unwrap(),
                    created: now,
                    updated: now,
                    asset_type: "web_page".parse::<SnakeCase>().unwrap(),
                    url: "https://example.org/article".parse::<HttpUrl>().unwrap(),
                    file_format: HtmlFileFormat::Html,
                    capture_method: "singlefile".parse::<SnakeCase>().unwrap(),
                    created_by_work_id: work.parse::<WorkId>().unwrap(),
                    related_work_ids: vec![],
                    content_hash: format!("{:x}", Sha256::digest(html))
                        .parse::<ContentHash>()
                        .unwrap(),
                    hash_algorithm: HashAlgorithm::Sha256,
                },
                html,
            )
            .unwrap();
        id
    }
}

#[test]
fn add_links_asset_saves_marker_and_verifies_quote() {
    let h = Harness::new();
    let work = h.work();
    let card = h.card(&work);
    let asset = h.asset(
        &work,
        b"<html><body><p>Alpha citation Omega</p></body></html>",
    );
    let result = h.ok(&[
        "card", "evidence", "add", &card, "--asset", &asset, "--quote", "citation",
    ]);
    assert!(result.contains("ev1"));
    let json = h.show(&card);
    assert_eq!(json["source_asset_ids"], json!([asset]));
    assert_eq!(json["evidence"][0]["id"], "ev1");
    assert_eq!(json["evidence"][0]["selector"]["exact"], "citation");
    assert_eq!(json["evidence"][0]["position_selector"]["start"], 6);
    assert_eq!(json["body"], "[^ev1]");
    assert!(
        h.ok(&["card", "evidence", "verify", &card])
            .contains("ev1: verified")
    );
    assert!(
        h.ok(&["card", "evidence", "list", &card])
            .contains("citation")
    );
    let show: Value =
        serde_json::from_str(&h.ok(&["card", "evidence", "show", &card, "ev1", "--json"])).unwrap();
    assert_eq!(show["asset_id"], asset);
}

#[test]
fn repeated_quotes_require_a_disambiguating_start() {
    let h = Harness::new();
    let work = h.work();
    let card = h.card(&work);
    let asset = h.asset(&work, b"<p>target and target</p>");
    assert!(
        h.fail(&[
            "card", "evidence", "add", &card, "--asset", &asset, "--quote", "target"
        ])
        .contains("ambiguous quote")
    );
    assert_eq!(h.show(&card)["evidence"], json!([]));
    h.ok(&[
        "card", "evidence", "add", &card, "--asset", &asset, "--quote", "target", "--start", "11",
    ]);
    assert_eq!(
        h.show(&card)["evidence"][0]["position_selector"]["start"],
        11
    );
    assert!(
        h.fail(&[
            "card", "evidence", "add", &card, "--asset", &asset, "--quote", "target", "--start",
            "9"
        ])
        .contains("codepoint start")
    );
    assert_eq!(h.show(&card)["evidence"].as_array().unwrap().len(), 1);
}

#[test]
fn successive_evidence_ids_and_markers_are_unique() {
    let h = Harness::new();
    let work = h.work();
    let card = h.card(&work);
    let asset = h.asset(&work, b"<p>alpha beta gamma</p>");
    for quote in ["alpha", "beta", "gamma"] {
        h.ok(&[
            "card", "evidence", "add", &card, "--asset", &asset, "--quote", quote,
        ]);
    }
    let json = h.show(&card);
    assert_eq!(json["evidence"].as_array().unwrap().len(), 3);
    assert_eq!(json["evidence"][2]["id"], "ev3");
    assert_eq!(json["source_asset_ids"], json!([asset]));
    assert_eq!(json["body"], "[^ev1]\n\n[^ev2]\n\n[^ev3]");
}

#[test]
fn marker_offset_is_unicode_codepoint_based_and_grapheme_safe() {
    let h = Harness::new();
    let work = h.work();
    let card = h.card(&work);
    let asset = h.asset(&work, "<p>引用 😀 検証</p>".as_bytes());
    let path = h.root().join("markdown.md");
    fs::write(&path, "cafe\u{301} 📝").unwrap();
    h.ok(&["card", "edit", &card, "--body-file", path.to_str().unwrap()]);
    assert!(
        h.fail(&[
            "card",
            "evidence",
            "add",
            &card,
            "--asset",
            &asset,
            "--quote",
            "引用",
            "--marker-offset",
            "4"
        ])
        .contains("grapheme")
    );
    assert_eq!(h.show(&card)["evidence"], json!([]));
    h.ok(&[
        "card",
        "evidence",
        "add",
        &card,
        "--asset",
        &asset,
        "--quote",
        "引用",
        "--marker-offset",
        "5",
    ]);
    assert_eq!(h.show(&card)["body"], "cafe\u{301}[^ev1] 📝");
}

#[test]
fn failed_validation_does_not_modify_card_bytes() {
    let h = Harness::new();
    let work = h.work();
    let card = h.card(&work);
    let asset = h.asset(&work, b"<p>original</p>");
    let path = h.root().join("cards").join(format!("{card}.json"));
    let before = fs::read(&path).unwrap();
    for quote in ["missing", " "] {
        assert!(
            h.fail(&[
                "card", "evidence", "add", &card, "--asset", &asset, "--quote", quote
            ])
            .contains("error:")
        );
    }
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(
        h.fail(&[
            "card", "evidence", "add", &card, "--asset", "unknown0", "--quote", "original"
        ])
        .contains("file not found")
    );
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn modified_html_hash_is_rejected_without_mutating_card() {
    let h = Harness::new();
    let work = h.work();
    let card = h.card(&work);
    let asset = h.asset(&work, b"<p>original</p>");
    fs::write(
        h.root().join("assets").join(format!("{asset}.html")),
        b"<p>forged</p>",
    )
    .unwrap();
    assert!(
        h.fail(&[
            "card", "evidence", "add", &card, "--asset", &asset, "--quote", "forged"
        ])
        .contains("SHA-256 mismatch")
    );
    assert_eq!(h.show(&card)["evidence"], json!([]));
}

#[test]
fn verifies_and_reports_recovered_and_unresolved_positions() {
    let h = Harness::new();
    let work = h.work();
    let card = h.card(&work);
    let asset = h.asset(&work, b"<p>alpha citation omega</p>");
    h.ok(&[
        "card", "evidence", "add", &card, "--asset", &asset, "--quote", "citation",
    ]);
    let id: CardId = card.parse().unwrap();
    let repo = h.repo();
    let loaded = repo.load_card(&id).unwrap();
    repo.update_card(&id, &loaded.revision, |card| {
        card.evidence[0].position_selector.start = 0;
        card.evidence[0].position_selector.end = 8;
    })
    .unwrap();
    assert_eq!(
        repo.verify_card_evidence(&id).unwrap()[0].1,
        Resolution::Recovered(6)
    );
    assert!(
        h.fail(&["card", "evidence", "verify", &card])
            .contains("require review")
    );
    let loaded = repo.load_card(&id).unwrap();
    repo.update_card(&id, &loaded.revision, |card| {
        card.evidence[0].selector.prefix = "forged prefix".into();
    })
    .unwrap();
    assert_eq!(
        repo.verify_card_evidence(&id).unwrap()[0].1,
        Resolution::Unresolved
    );
}

#[test]
fn rejects_stale_card_revision_and_preserves_newer_edit() {
    let h = Harness::new();
    let work = h.work();
    let card = h.card(&work);
    let asset = h.asset(&work, b"<p>citation</p>");
    let id: CardId = card.parse().unwrap();
    let asset_id: AssetId = asset.parse().unwrap();
    let repo = h.repo();
    let stale = repo.load_card(&id).unwrap();
    repo.update_card(&id, &stale.revision, |card| {
        card.body = "newer content".into();
    })
    .unwrap();
    assert!(matches!(
        repo.add_card_evidence(&id, &stale.revision, &asset_id, "citation", None, None),
        Err(RepositoryError::Conflict(_))
    ));
    let after = repo.load_card(&id).unwrap().value;
    assert!(after.evidence.is_empty());
    assert_eq!(after.body, "newer content");
}

#[test]
fn cannot_remove_a_source_asset_referenced_by_evidence() {
    let h = Harness::new();
    let work = h.work();
    let card = h.card(&work);
    let asset = h.asset(&work, b"<p>citation</p>");
    h.ok(&[
        "card", "evidence", "add", &card, "--asset", &asset, "--quote", "citation",
    ]);
    assert!(
        h.fail(&["card", "source", "asset", "remove", &card, &asset])
            .contains("missing from source_asset_ids")
    );
}

#[test]
fn invalid_utf8_snapshot_is_rejected() {
    let h = Harness::new();
    let work = h.work();
    let card = h.card(&work);
    let asset = h.asset(&work, &[0xff, 0xfe]);
    assert!(
        h.fail(&[
            "card", "evidence", "add", &card, "--asset", &asset, "--quote", "test"
        ])
        .contains("not UTF-8")
    );
    assert_eq!(h.show(&card)["evidence"], json!([]));
}
