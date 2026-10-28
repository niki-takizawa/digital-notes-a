//! `digital-notes check`: report all recoverable integrity issues and exit 1.
//! No browser, network or real SingleFile executable is needed.

use digital_notes::{
    model::{
        Asset, AssetEntityType, AssetId, ContentHash, HashAlgorithm, HtmlFileFormat, HttpUrl,
        SchemaVersion, SnakeCase, Title, UtcTimestamp, WorkId,
    },
    repository::Repository,
};
use serde_json::Value;
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
        let test = Self {
            temp: tempfile::tempdir().unwrap(),
        };
        test.ok(&["init"]);
        test
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

    fn work(&self) -> String {
        self.ok(&["work", "new", "--title", "Research", "--id-only"])
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

    fn asset(&self, work: &str, html: &[u8]) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let now = UtcTimestamp::now();
        self.repo()
            .create_asset_pair(
                Asset {
                    schema_version: SchemaVersion,
                    id: id.parse::<AssetId>().unwrap(),
                    entity_type: AssetEntityType::Asset,
                    title: "Source".parse::<Title>().unwrap(),
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

    fn report(&self) -> (Output, Value) {
        let result = self.invoke(&["check", "--json"]);
        let report: Value = serde_json::from_slice(&result.stdout).unwrap();
        (result, report)
    }
}

fn has_kind(report: &Value, kind: &str) -> bool {
    report["issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|issue| issue["kind"] == kind)
}

#[test]
fn fresh_repository_passes_and_is_reported_as_empty_json() {
    let h = Harness::new();
    assert!(h.ok(&["check"]).contains("OK:"));
    let (result, report) = h.report();
    assert!(result.status.success());
    assert_eq!(report["ok"], true);
    assert_eq!(report["issue_count"], 0);
    assert_eq!(report["issues"].as_array().unwrap().len(), 0);
}

#[test]
fn valid_entities_without_evidence_pass() {
    let h = Harness::new();
    let work = h.work();
    h.card(&work);
    h.asset(&work, b"<html><body>Source</body></html>");
    assert!(h.invoke(&["check"]).status.success());
}

#[test]
fn reports_broken_reference_and_keeps_checking_other_issues() {
    let h = Harness::new();
    let work = h.work();
    let card = h.card(&work);
    let missing_work = h.work();
    fs::remove_file(h.root().join("works").join(format!("{missing_work}.json"))).unwrap();
    let other = h.card(&work);
    let id = other.parse().unwrap();
    let repo = h.repo();
    let loaded = repo.load_card(&id).unwrap();
    repo.update_card(&id, &loaded.revision, |card| {
        card.source_work_ids.push(missing_work.parse().unwrap());
    })
    .unwrap();
    fs::write(h.root().join("assets/asset1234.html"), b"<html></html>").unwrap();
    let (result, report) = h.report();
    assert!(!result.status.success());
    assert!(has_kind(&report, "missing_work"));
    assert!(has_kind(&report, "orphan_asset_html"));
    assert_eq!(report["ok"], false);
    assert!(report["issue_count"].as_u64().unwrap() >= 2);
    assert!(!card.is_empty());
}

#[test]
fn missing_or_tampered_asset_html_is_detected() {
    let h = Harness::new();
    let work = h.work();
    let missing = h.asset(&work, b"<p>one</p>");
    let tampered = h.asset(&work, b"<p>two</p>");
    fs::remove_file(h.root().join(format!("assets/{missing}.html"))).unwrap();
    fs::write(
        h.root().join(format!("assets/{tampered}.html")),
        b"<p>changed</p>",
    )
    .unwrap();
    let (_, report) = h.report();
    assert!(has_kind(&report, "missing_asset_html"));
    assert!(has_kind(&report, "invalid_entity"));
}

#[test]
fn invalid_card_json_does_not_prevent_other_cards_from_being_checked() {
    let h = Harness::new();
    let work = h.work();
    let card = h.card(&work);
    let asset = h.asset(&work, b"<p>alpha citation omega</p>");
    h.ok(&[
        "card", "evidence", "add", &card, "--asset", &asset, "--quote", "citation",
    ]);
    fs::write(h.root().join("cards/badcard1.json"), b"{not json").unwrap();
    let (_, report) = h.report();
    assert!(has_kind(&report, "invalid_entity"));
    assert!(!has_kind(&report, "evidence_unresolved"));
}

#[test]
fn reports_recovered_and_unresolved_evidence() {
    let h = Harness::new();
    let work = h.work();
    let card1 = h.card(&work);
    let card2 = h.card(&work);
    let asset = h.asset(&work, b"<p>alpha citation omega</p>");
    for card in [&card1, &card2] {
        h.ok(&[
            "card", "evidence", "add", card, "--asset", &asset, "--quote", "citation",
        ]);
    }
    let repo = h.repo();
    let id1 = card1.parse().unwrap();
    let loaded = repo.load_card(&id1).unwrap();
    repo.update_card(&id1, &loaded.revision, |card| {
        card.evidence[0].position_selector.start = 0;
        card.evidence[0].position_selector.end = 8;
    })
    .unwrap();
    let id2 = card2.parse().unwrap();
    let loaded = repo.load_card(&id2).unwrap();
    repo.update_card(&id2, &loaded.revision, |card| {
        card.evidence[0].selector.prefix = "forged".into();
    })
    .unwrap();
    let (result, report) = h.report();
    assert!(!result.status.success());
    assert!(has_kind(&report, "evidence_position_changed"));
    assert!(has_kind(&report, "evidence_unresolved"));
}

#[test]
fn evidence_lookup_error_is_reported_not_treated_as_a_fatal_scan_error() {
    let h = Harness::new();
    let work = h.work();
    let card = h.card(&work);
    let asset = h.asset(&work, b"<p>citation</p>");
    h.ok(&[
        "card", "evidence", "add", &card, "--asset", &asset, "--quote", "citation",
    ]);
    fs::remove_file(h.root().join(format!("assets/{asset}.html"))).unwrap();
    let (result, report) = h.report();
    assert!(!result.status.success());
    assert!(has_kind(&report, "missing_asset_html"));
    assert!(has_kind(&report, "evidence_verification_error"));
}

#[test]
fn text_output_uses_deterministic_relative_paths_and_nonzero_status() {
    let h = Harness::new();
    fs::write(h.root().join("assets/asset1234.html"), b"<html></html>").unwrap();
    let result = h.invoke(&["check"]);
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8(result.stdout).unwrap();
    assert!(
        stdout.contains("[orphan_asset_html] assets/asset1234.html"),
        "{stdout}"
    );
    assert!(String::from_utf8_lossy(&result.stderr).contains("integrity check failed"));
}
