use digital_notes::model::{Asset, Card, Work, WorkStatus};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

fn roundtrip<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(value: Value) {
    let typed: T = serde_json::from_value(value.clone()).expect("valid input");
    let serialized = serde_json::to_value(&typed).expect("serialize");
    assert_eq!(serialized, value);
    assert_eq!(
        serde_json::from_value::<T>(serialized).expect("deserialize serialized value"),
        typed
    );
}

fn work() -> Value {
    json!({
        "schema_version": 1,
        "id": "work0001",
        "entity_type": "work",
        "title": "Collect research papers",
        "created": "2026-10-28T10:00:00Z",
        "updated": "2026-10-28T10:00:00Z",
        "work_type": "source_collection",
        "status": "active",
        "source_work_ids": [],
        "body": "## Search strategy\n"
    })
}

fn asset() -> Value {
    json!({
        "schema_version": 1,
        "id": "asset001",
        "entity_type": "asset",
        "title": "Example publication",
        "created": "2026-10-28T10:10:00Z",
        "updated": "2026-10-28T10:10:00Z",
        "asset_type": "web_page",
        "url": "https://example.org/paper",
        "file_format": "html",
        "capture_method": "singlefile",
        "created_by_work_id": "work0001",
        "related_work_ids": ["work0002"],
        "content_hash": "a".repeat(64),
        "hash_algorithm": "sha256"
    })
}

fn card() -> Value {
    json!({
        "schema_version": 1,
        "id": "card0001",
        "entity_type": "card",
        "title": "Research notes",
        "created": "2026-10-28T10:20:00Z",
        "updated": "2026-10-28T10:20:00Z",
        "card_type": "note",
        "created_by_work_id": "work0001",
        "source_work_ids": ["work0002"],
        "source_asset_ids": ["asset001", "asset002"],
        "source_card_ids": [],
        "evidence": [{
            "id": "ev1",
            "asset_id": "asset001",
            "text_normalization": "html_text_v1",
            "selector": {
                "type": "TextQuoteSelector",
                "exact": "A quote",
                "prefix": "Some ",
                "suffix": " from the page"
            },
            "position_selector": {
                "type": "TextPositionSelector",
                "start": 5,
                "end": 12
            }
        }],
        "body": "A note with reference [^ev1]."
    })
}

#[test]
fn work_roundtrip() {
    roundtrip::<Work>(work());
}

#[test]
fn asset_roundtrip() {
    roundtrip::<Asset>(asset());
}

#[test]
fn card_roundtrip_with_multiple_assets_and_evidence() {
    roundtrip::<Card>(card());
}

#[test]
fn work_status_roundtrip() {
    let mut work = serde_json::from_value::<Work>(work()).expect("work");
    work.status = WorkStatus::Done;
    assert_eq!(serde_json::to_value(&work).unwrap()["status"], "done");
}

#[test]
fn missing_and_unknown_fields_fail() {
    let mut missing = work();
    missing.as_object_mut().unwrap().remove("updated");
    assert!(serde_json::from_value::<Work>(missing).is_err());

    let mut extra = card();
    extra["unexpected"] = json!(true);
    assert!(serde_json::from_value::<Card>(extra).is_err());
}

#[test]
fn invalid_literals_fail() {
    let mut wrong_status = work();
    wrong_status["status"] = json!("paused");
    assert!(serde_json::from_value::<Work>(wrong_status).is_err());

    let mut wrong_entity = asset();
    wrong_entity["entity_type"] = json!("card");
    assert!(serde_json::from_value::<Asset>(wrong_entity).is_err());

    let mut wrong_version = card();
    wrong_version["schema_version"] = json!(2);
    assert!(serde_json::from_value::<Card>(wrong_version).is_err());
}
