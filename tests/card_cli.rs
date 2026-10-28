//! End-to-end Card CLI tests. All data is confined to temporary repositories.
use digital_notes::{
    model::{
        Asset, AssetEntityType, AssetId, ContentHash, Evidence, HashAlgorithm, HtmlFileFormat,
        HttpUrl, SchemaVersion, SnakeCase, TextNormalization, TextPositionSelector,
        TextPositionSelectorType, TextQuoteSelector, TextQuoteSelectorType, Title, UtcTimestamp,
        WorkId,
    },
    repository::Repository,
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
        let h = Self {
            temp: tempfile::tempdir().unwrap(),
        };
        h.success(&["init"]);
        h
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

    fn success(&self, args: &[&str]) -> String {
        let output = self.invoke(args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    fn failure(&self, args: &[&str]) -> String {
        let output = self.invoke(args);
        assert!(!output.status.success(), "unexpected success for {args:?}");
        String::from_utf8(output.stderr).unwrap()
    }

    fn work(&self, title: &str) -> String {
        self.success(&["work", "new", "--title", title, "--id-only"])
            .trim()
            .to_owned()
    }

    fn card(&self, title: &str, work_id: &str) -> String {
        self.success(&[
            "card",
            "new",
            "--title",
            title,
            "--work",
            work_id,
            "--id-only",
        ])
        .trim()
        .to_owned()
    }

    fn show(&self, id: &str) -> Value {
        serde_json::from_str(&self.success(&["card", "show", id, "--json"])).unwrap()
    }

    fn asset(&self, work_id: &str, title: &str) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let html = b"<!doctype html><title>Reference</title><p>Source text</p>";
        let hash = format!("{:x}", Sha256::digest(html));
        let now = UtcTimestamp::now();
        self.repo()
            .create_asset_pair(
                Asset {
                    schema_version: SchemaVersion,
                    id: id.parse::<AssetId>().unwrap(),
                    entity_type: AssetEntityType::Asset,
                    title: title.parse::<Title>().unwrap(),
                    created: now,
                    updated: now,
                    asset_type: "web_page".parse::<SnakeCase>().unwrap(),
                    url: "https://example.org/page".parse::<HttpUrl>().unwrap(),
                    file_format: HtmlFileFormat::Html,
                    capture_method: "singlefile".parse::<SnakeCase>().unwrap(),
                    created_by_work_id: work_id.parse::<WorkId>().unwrap(),
                    related_work_ids: Vec::new(),
                    content_hash: hash.parse::<ContentHash>().unwrap(),
                    hash_algorithm: HashAlgorithm::Sha256,
                },
                html,
            )
            .unwrap();
        id
    }
}

#[test]
fn new_card_requires_a_work_and_preserves_the_schema_contract() {
    let h = Harness::new();
    assert_eq!(h.success(&["card", "list", "--json"]).trim(), "[]");
    let work_id = h.work("Collect sources");
    let body = h.root().join("draft.md");
    fs::write(&body, "# Analysis\n\nInitial notes 📝\n").unwrap();
    let id = h.success(&[
        "card",
        "new",
        "--title",
        "First interpretation",
        "--work",
        &work_id,
        "--type",
        "summary",
        "--body-file",
        body.to_str().unwrap(),
        "--id-only",
    ]);
    let id = id.trim();
    assert_eq!(id.len(), 36);
    let card = h.show(id);
    assert_eq!(card["schema_version"], 1);
    assert_eq!(card["entity_type"], "card");
    assert_eq!(card["created_by_work_id"], work_id);
    assert_eq!(card["card_type"], "summary");
    assert_eq!(card["title"], "First interpretation");
    assert_eq!(card["created"], card["updated"]);
    assert_eq!(card["evidence"], json!([]));
    assert_eq!(card["source_asset_ids"], json!([]));
    assert_eq!(card["body"], "# Analysis\n\nInitial notes 📝\n");
    assert!(h.success(&["card", "show", id]).contains("Collect sources"));
    assert!(h.root().join("cards").join(format!("{id}.json")).exists());
}

#[test]
fn list_is_title_first_and_disambiguates_duplicates() {
    let h = Harness::new();
    let work = h.work("Owner");
    let zebra = h.card("Zebra", &work);
    let alpha = h.card("Alpha", &work);
    let again = h.card("Alpha", &work);
    let output = h.success(&["card", "list"]);
    assert!(output.starts_with("Alpha"), "{output}");
    assert!(output.contains(&format!("Alpha [{alpha}]")));
    assert!(output.contains(&format!("Alpha [{again}]")));
    assert!(!output.contains(&format!("Zebra [{zebra}]")));
    assert!(
        h.success(&["card", "list", "--ids"])
            .contains(&format!("Zebra [{zebra}]"))
    );
    let list: Value = serde_json::from_str(&h.success(&["card", "list", "--json"])).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 3);
}

#[test]
fn edit_and_set_only_change_editable_fields_and_noop_preserves_bytes() {
    let h = Harness::new();
    let owner = h.work("Owner");
    let card = h.card("Draft", &owner);
    let before = h.show(&card);
    h.success(&[
        "card",
        "set",
        &card,
        "--title",
        "Polished",
        "--type",
        "literature_review",
    ]);
    let body_file = h.root().join("body.md");
    fs::write(&body_file, "A *Markdown* analysis.\n").unwrap();
    h.success(&[
        "card",
        "edit",
        &card,
        "--body-file",
        body_file.to_str().unwrap(),
    ]);
    let after = h.show(&card);
    assert_eq!(after["title"], "Polished");
    assert_eq!(after["card_type"], "literature_review");
    assert_eq!(after["body"], "A *Markdown* analysis.\n");
    assert_eq!(after["id"], before["id"]);
    assert_eq!(after["created"], before["created"]);
    assert_eq!(after["created_by_work_id"], owner);
    let path = h.root().join("cards").join(format!("{card}.json"));
    let bytes = fs::read(&path).unwrap();
    h.success(&[
        "card",
        "set",
        &card,
        "--title",
        "Polished",
        "--type",
        "literature_review",
    ]);
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn sources_support_works_multiple_assets_and_other_cards() {
    let h = Harness::new();
    let owner = h.work("Owner");
    let other_work = h.work("Related task");
    let asset_a = h.asset(&owner, "Paper A");
    let asset_b = h.asset(&owner, "Paper B");
    let source_card = h.card("Preliminary interpretation", &owner);
    let card = h.success(&[
        "card",
        "new",
        "--title",
        "Synthesis",
        "--work",
        &owner,
        "--source-work",
        &other_work,
        "--source-asset",
        &asset_a,
        "--source-asset",
        &asset_b,
        "--source-card",
        &source_card,
        "--id-only",
    ]);
    let card = card.trim();
    let initial = h.show(card);
    assert_eq!(initial["source_work_ids"], json!([other_work]));
    assert_eq!(initial["source_asset_ids"], json!([asset_a, asset_b]));
    assert_eq!(initial["source_card_ids"], json!([source_card]));
    let output = h.success(&["card", "show", card]);
    for title in [
        "Owner",
        "Related task",
        "Paper A",
        "Paper B",
        "Preliminary interpretation",
    ] {
        assert!(output.contains(title), "missing {title}: {output}");
    }
    h.success(&["card", "source", "work", "add", card, &other_work]);
    h.success(&["card", "source", "asset", "add", card, &asset_a]);
    h.success(&["card", "source", "card", "add", card, &source_card]);
    assert_eq!(h.show(card)["source_asset_ids"], json!([asset_a, asset_b]));
    h.success(&["card", "source", "asset", "remove", card, &asset_a]);
    h.success(&["card", "source", "work", "remove", card, &other_work]);
    h.success(&["card", "source", "card", "remove", card, &source_card]);
    assert_eq!(h.show(card)["source_asset_ids"], json!([asset_b]));
    assert_eq!(h.show(card)["source_work_ids"], json!([]));
    assert_eq!(h.show(card)["source_card_ids"], json!([]));
}

#[test]
fn invalid_inputs_are_rejected_without_creating_cards_or_dropping_sources() {
    let h = Harness::new();
    let owner = h.work("Owner");
    let card = h.card("Main Card", &owner);
    assert!(
        h.failure(&["card", "new", "--title", "Bad", "--work", "missing0"])
            .contains("error:")
    );
    assert!(
        h.failure(&["card", "new", "--title", "  ", "--work", &owner])
            .contains("error:")
    );
    assert!(
        h.failure(&[
            "card", "new", "--title", "Bad", "--work", &owner, "--type", "NotSnake"
        ])
        .contains("error:")
    );
    assert!(
        h.failure(&[
            "card",
            "new",
            "--title",
            "Bad",
            "--work",
            &owner,
            "--source-asset",
            "missing0"
        ])
        .contains("error:")
    );
    assert!(
        h.failure(&[
            "card",
            "new",
            "--title",
            "Bad",
            "--work",
            &owner,
            "--source-card",
            "missing0"
        ])
        .contains("error:")
    );
    assert!(
        h.failure(&[
            "card",
            "new",
            "--title",
            "Bad",
            "--work",
            &owner,
            "--source-work",
            &owner
        ])
        .contains("invalid Card")
    );
    assert!(
        h.failure(&["card", "set", &card])
            .contains("specify --title")
    );
    assert!(
        h.failure(&["card", "source", "work", "add", &card, &owner])
            .contains("invalid Card")
    );
    assert!(
        h.failure(&["card", "source", "card", "add", &card, &card])
            .contains("invalid Card")
    );
    assert!(
        h.failure(&["card", "source", "asset", "add", &card, "missing0"])
            .contains("error:")
    );
    assert!(
        h.failure(&["card", "source", "card", "remove", &card, "missing0"])
            .contains("not referenced")
    );
    assert!(
        h.failure(&["card", "source", "work", "remove", &card, "missing0"])
            .contains("not referenced")
    );
    assert!(
        h.failure(&["card", "source", "asset", "remove", &card, "missing0"])
            .contains("not referenced")
    );
    let cards: Value = serde_json::from_str(&h.success(&["card", "list", "--json"])).unwrap();
    assert_eq!(cards.as_array().unwrap().len(), 1);
    assert_eq!(h.show(&card)["source_card_ids"], json!([]));
}

#[test]
fn evidence_prevents_removal_of_its_source_asset() {
    let h = Harness::new();
    let owner = h.work("Owner");
    let asset = h.asset(&owner, "Source text");
    let card = h
        .success(&[
            "card",
            "new",
            "--title",
            "Citations",
            "--work",
            &owner,
            "--source-asset",
            &asset,
            "--id-only",
        ])
        .trim()
        .to_owned();
    let id = card.parse().unwrap();
    let repo = h.repo();
    let loaded = repo.load_card(&id).unwrap();
    repo.update_card(&id, &loaded.revision, |card| {
        card.evidence.push(Evidence {
            id: "ev1".parse().unwrap(),
            asset_id: asset.parse().unwrap(),
            text_normalization: TextNormalization::HtmlTextV1,
            selector: TextQuoteSelector {
                selector_type: TextQuoteSelectorType::TextQuoteSelector,
                exact: "Source".to_owned(),
                prefix: String::new(),
                suffix: " text".to_owned(),
            },
            position_selector: TextPositionSelector {
                selector_type: TextPositionSelectorType::TextPositionSelector,
                start: 0,
                end: 6,
            },
        });
    })
    .unwrap();
    assert!(
        h.failure(&["card", "source", "asset", "remove", &card, &asset])
            .contains("missing from source_asset_ids")
    );
    assert_eq!(h.show(&card)["source_asset_ids"], json!([asset]));
    assert_eq!(h.show(&card)["evidence"][0]["id"], "ev1");
}

#[test]
fn damaged_card_is_not_silently_skipped_in_list() {
    let h = Harness::new();
    let owner = h.work("Owner");
    let card = h.card("Good", &owner);
    fs::write(
        h.root().join("cards").join(format!("{card}.json")),
        "{bad json",
    )
    .unwrap();
    assert!(h.failure(&["card", "list"]).contains("invalid JSON"));
}

#[cfg(unix)]
#[test]
fn external_editor_round_trip_and_editor_failure_preserves_original() {
    use std::os::unix::fs::PermissionsExt;
    let h = Harness::new();
    let owner = h.work("Owner");
    let card = h.card("Editing", &owner);
    let editor = h.root().join("edit-card.sh");
    fs::write(
        &editor,
        "#!/bin/sh\nprintf 'From external editor\\n' >> \"$1\"\n",
    )
    .unwrap();
    let mut permissions = fs::metadata(&editor).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&editor, permissions).unwrap();
    let output = Command::new(BIN)
        .arg("--root")
        .arg(h.root())
        .args(["card", "edit", &card])
        .env("VISUAL", editor.as_os_str())
        .env_remove("EDITOR")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(h.show(&card)["body"], "From external editor\n");
    fs::write(&editor, "#!/bin/sh\nprintf 'Unsaved' >> \"$1\"\nexit 5\n").unwrap();
    let failed = Command::new(BIN)
        .arg("--root")
        .arg(h.root())
        .args(["card", "edit", &card])
        .env("VISUAL", editor.as_os_str())
        .env_remove("EDITOR")
        .output()
        .unwrap();
    assert!(!failed.status.success());
    let stderr = String::from_utf8_lossy(&failed.stderr);
    assert!(stderr.contains("preserved at"), "{stderr}");
    assert_eq!(h.show(&card)["body"], "From external editor\n");
    if let Some(path) = stderr.trim().split("preserved at ").nth(1) {
        fs::remove_file(path).unwrap();
    }
}
