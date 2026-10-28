//! Structural agreement between the JSON fixtures, Schema v1 and serde models.
//! The complete JSON Schema constraints are exercised by test_schema_contract.py.

use digital_notes::model::{Asset, Card, Work};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn keys(object: &Value) -> BTreeSet<&str> {
    object
        .as_object()
        .expect("JSON object")
        .keys()
        .map(String::as_str)
        .collect()
}

fn schema_required(schema: &Value) -> BTreeSet<&str> {
    schema["required"]
        .as_array()
        .expect("required fields")
        .iter()
        .map(|field| field.as_str().expect("field name"))
        .collect()
}

fn assert_surface<T>(schema_json: &str, fixture_json: &str)
where
    T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let schema: Value = serde_json::from_str(schema_json).expect("valid schema JSON");
    let fixture: Value = serde_json::from_str(fixture_json).expect("valid fixture JSON");
    let fields = keys(&fixture);

    assert_eq!(
        fields,
        keys(&schema["properties"]),
        "fixture vs schema fields"
    );
    assert_eq!(
        fields,
        schema_required(&schema),
        "all fields must be required"
    );
    assert_eq!(schema["additionalProperties"], json!(false));

    let model: T = serde_json::from_value(fixture.clone()).expect("fixture deserializes");
    let serialized = serde_json::to_value(&model).expect("model serializes");
    assert_eq!(serialized, fixture, "Rust model preserves fixture");

    for missing_field in fields {
        let mut invalid = fixture.clone();
        invalid
            .as_object_mut()
            .expect("fixture object")
            .remove(missing_field);
        assert!(
            serde_json::from_value::<T>(invalid).is_err(),
            "Rust accepted missing required field {missing_field}"
        );
    }

    let mut extra = fixture.clone();
    extra["unexpected_field"] = json!(true);
    assert!(
        serde_json::from_value::<T>(extra).is_err(),
        "Rust accepted an unknown field"
    );
}

#[test]
fn work_surface_matches_schema() {
    assert_surface::<Work>(
        include_str!("../schemas/work.schema.json"),
        include_str!("fixtures/work.json"),
    );
}

#[test]
fn asset_surface_matches_schema() {
    assert_surface::<Asset>(
        include_str!("../schemas/asset.schema.json"),
        include_str!("fixtures/asset.json"),
    );
}

#[test]
fn card_surface_matches_schema() {
    assert_surface::<Card>(
        include_str!("../schemas/card.schema.json"),
        include_str!("fixtures/card.json"),
    );
}

#[test]
fn nested_evidence_surface_matches_schema() {
    let schema: Value = serde_json::from_str(include_str!("../schemas/card.schema.json")).unwrap();
    let fixture: Value = serde_json::from_str(include_str!("fixtures/card.json")).unwrap();
    let evidence = &fixture["evidence"][0];
    let definition = &schema["$defs"]["evidence"];
    assert_eq!(keys(evidence), keys(&definition["properties"]));
    assert_eq!(keys(evidence), schema_required(definition));

    for (field, object, definition) in [
        (
            "selector",
            &evidence["selector"],
            &definition["properties"]["selector"],
        ),
        (
            "position_selector",
            &evidence["position_selector"],
            &definition["properties"]["position_selector"],
        ),
    ] {
        assert_eq!(keys(object), keys(&definition["properties"]), "{field}");
        assert_eq!(keys(object), schema_required(definition), "{field}");
    }

    for pointer in ["/evidence/0/selector", "/evidence/0/position_selector"] {
        let mut unexpected = fixture.clone();
        unexpected.pointer_mut(pointer).unwrap()["unexpected_field"] = json!(true);
        assert!(
            serde_json::from_value::<Card>(unexpected).is_err(),
            "{pointer}"
        );
    }
}

#[test]
fn shared_invalid_values_are_rejected_by_rust() {
    let work: Value = serde_json::from_str(include_str!("fixtures/work.json")).unwrap();
    let asset: Value = serde_json::from_str(include_str!("fixtures/asset.json")).unwrap();
    let card: Value = serde_json::from_str(include_str!("fixtures/card.json")).unwrap();

    for (key, replacement) in [
        ("id", json!("short")),
        ("status", json!("paused")),
        ("work_type", json!("NotSnakeCase")),
        ("created", json!("2026-02-30T10:00:00Z")),
        ("schema_version", json!(2)),
    ] {
        let mut invalid = work.clone();
        invalid[key] = replacement;
        assert!(serde_json::from_value::<Work>(invalid).is_err(), "{key}");
    }
    for (key, replacement) in [
        ("content_hash", json!("not-a-sha256")),
        ("file_format", json!("pdf")),
        ("hash_algorithm", json!("sha1")),
        ("url", json!("file:///etc/passwd")),
    ] {
        let mut invalid = asset.clone();
        invalid[key] = replacement;
        assert!(serde_json::from_value::<Asset>(invalid).is_err(), "{key}");
    }
    for (pointer, replacement) in [
        ("/evidence/0/id", json!("ev0")),
        ("/evidence/0/selector/type", json!("OtherSelector")),
        ("/evidence/0/position_selector/start", json!(-1)),
    ] {
        let mut invalid = card.clone();
        *invalid.pointer_mut(pointer).unwrap() = replacement;
        assert!(
            serde_json::from_value::<Card>(invalid).is_err(),
            "{pointer}"
        );
    }
}
