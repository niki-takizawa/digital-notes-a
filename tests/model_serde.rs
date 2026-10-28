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
    serde_json::from_str(include_str!("fixtures/work.json")).expect("work fixture")
}

fn asset() -> Value {
    serde_json::from_str(include_str!("fixtures/asset.json")).expect("asset fixture")
}

fn card() -> Value {
    serde_json::from_str(include_str!("fixtures/card.json")).expect("card fixture")
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
