//! Contract tests for versioned DOM text extraction and Evidence positions.
use digital_notes::{
    evidence::{self, EvidenceError, Resolution},
    model::{AssetId, EvidenceId},
    text::{self, TextError},
};
use serde_json::Value;

fn asset_id() -> AssetId {
    "asset0001".parse().unwrap()
}

fn evidence_id() -> EvidenceId {
    "ev1".parse().unwrap()
}

#[test]
fn html_text_v1_matches_old_pri_golden_vectors() {
    let vectors: Value = serde_json::from_str(include_str!("fixtures/html_text_v1.json")).unwrap();
    for case in vectors.as_array().unwrap() {
        let html = case["html"].as_str().unwrap();
        let expected = case["expected"].as_str().unwrap();
        assert_eq!(
            text::html_text_v1(html.as_bytes()).unwrap(),
            expected,
            "{}",
            case["name"]
        );
    }
}

#[test]
fn html_excludes_non_text_elements_and_hidden_attribute() {
    let html = br#"<!doctype html><html><head><title>HEAD</title></head><body>
        <p>A <span>bold</span><script>SECRET1</script><style>SECRET2</style>
        <span hidden>SECRET3</span> B</p><p>Next</p>
        </body></html>"#;
    assert_eq!(text::html_text_v1(html).unwrap(), "A bold B Next");
}

#[test]
fn invalid_utf8_is_rejected() {
    assert_eq!(text::html_text_v1(&[0xff]), Err(TextError::InvalidUtf8));
}

#[test]
fn whitespace_rules_are_versioned_not_unicode_normalization() {
    assert_eq!(text::normalize_whitespace("\u{00a0} a\t b\n"), "a b");
    assert_eq!(text::normalize_whitespace("cafe\u{301}"), "cafe\u{301}");
    assert_ne!(text::normalize_whitespace("café"), "cafe\u{301}");
    assert!(!text::is_separator('\u{200b}'));
}

#[test]
fn offsets_are_codepoints_not_utf8_bytes() {
    let body = "甲😀 cafe\u{301} 乙";
    let ev = evidence::make_evidence(evidence_id(), asset_id(), body, "😀", None).unwrap();
    assert_eq!(
        (ev.position_selector.start, ev.position_selector.end),
        (1, 2)
    );
    assert_eq!(text::cp_to_byte(body, 1), Some(3));
    assert_eq!(text::slice_chars(body, 1, 2).as_deref(), Some("😀"));
    assert_eq!(text::slice_chars(body, 2, 1), None);
    assert_eq!(text::slice_chars(body, 0, usize::MAX), None);
    assert_eq!(
        text::cp_to_byte(body, body.chars().count()),
        Some(body.len())
    );
    assert_eq!(text::cp_to_byte(body, body.chars().count() + 1), None);
}

#[test]
fn quotes_may_not_split_a_grapheme_cluster() {
    let text = "cafe\u{301} 😀👍🏽";
    assert!(!text::grapheme_boundary(text, 4));
    assert!(evidence::quote_matches(text, "\u{301}").is_empty());
    assert_eq!(
        evidence::make_evidence(evidence_id(), asset_id(), text, "\u{301}", None),
        Err(EvidenceError::NotFound)
    );
    assert_eq!(evidence::quote_matches(text, "👍"), Vec::<usize>::new());
    assert_eq!(evidence::quote_matches(text, "👍🏽"), vec![7]);
}

#[test]
fn empty_missing_and_duplicate_quotes_fail_explicitly() {
    let body = "alpha target omega. beta target gamma";
    assert_eq!(
        evidence::make_evidence(evidence_id(), asset_id(), body, "  \n", None),
        Err(EvidenceError::EmptyQuote)
    );
    assert_eq!(
        evidence::make_evidence(evidence_id(), asset_id(), body, "none", None),
        Err(EvidenceError::NotFound)
    );
    assert_eq!(
        evidence::make_evidence(evidence_id(), asset_id(), body, "target", None),
        Err(EvidenceError::Ambiguous { matches: 2 })
    );
    assert_eq!(
        evidence::make_evidence(evidence_id(), asset_id(), body, "target", Some(7)),
        Err(EvidenceError::InvalidStart { start: 7 })
    );
}

#[test]
fn explicit_position_and_contexts_resolve_exactly() {
    let body = "alpha target omega. beta target gamma";
    let ev = evidence::make_evidence(evidence_id(), asset_id(), body, "target", Some(6)).unwrap();
    assert_eq!(ev.selector.exact, "target");
    assert_eq!(ev.selector.prefix, "alpha ");
    assert_eq!(ev.selector.suffix, " omega. beta target gamma");
    assert_eq!(ev.position_selector.end, 12);
    assert_eq!(evidence::resolve(body, &ev), Resolution::Verified);
    assert_eq!(
        evidence::resolve("alpha missing omega", &ev),
        Resolution::Unresolved
    );
}

#[test]
fn copied_newlines_normalize_to_single_space() {
    let body = "This quote spans two lines.";
    let ev =
        evidence::make_evidence(evidence_id(), asset_id(), body, "quote\nspans", None).unwrap();
    assert_eq!(ev.selector.exact, "quote spans");
    assert_eq!(evidence::resolve(body, &ev), Resolution::Verified);
}

#[test]
fn wrong_position_recovers_only_with_unique_context_match() {
    let body = "target at the beginning";
    let mut ev = evidence::make_evidence(evidence_id(), asset_id(), body, "target", None).unwrap();
    ev.position_selector.start = 11;
    ev.position_selector.end = 17;
    assert_eq!(evidence::resolve(body, &ev), Resolution::Recovered(0));

    let mut ambiguous =
        evidence::make_evidence(evidence_id(), asset_id(), "target", "target", None).unwrap();
    ambiguous.position_selector.start = 5;
    ambiguous.position_selector.end = 11;
    assert_eq!(
        evidence::resolve("targettarget", &ambiguous),
        Resolution::Unresolved
    );
}

#[test]
fn tampered_quote_or_context_cannot_verify() {
    let body = "one two three";
    let mut ev = evidence::make_evidence(evidence_id(), asset_id(), body, "two", None).unwrap();
    ev.selector.prefix = "wrong".into();
    assert_eq!(evidence::resolve(body, &ev), Resolution::Unresolved);
    ev.selector.prefix = "one ".into();
    ev.selector.exact = "three".into();
    assert_eq!(evidence::resolve(body, &ev), Resolution::Unresolved);
}

#[test]
fn saved_selector_round_trips_as_schema_field_names() {
    let ev = evidence::make_evidence(evidence_id(), asset_id(), "a😀b", "😀", None).unwrap();
    let json = serde_json::to_value(&ev).unwrap();
    assert_eq!(json["text_normalization"], "html_text_v1");
    assert_eq!(json["selector"]["type"], "TextQuoteSelector");
    assert_eq!(json["position_selector"]["type"], "TextPositionSelector");
    assert_eq!(json["position_selector"]["start"], 1);
    let decoded: digital_notes::model::Evidence = serde_json::from_value(json).unwrap();
    assert_eq!(decoded, ev);
}
