use super::super::canonical;
use crate::record::{EvidenceRef, StoredRecord};
use serde_json::{Value, json};
use uuid::Uuid;

/// Two payloads that differ only in the order their keys were written are the
/// same record to a reader, so they must hash the same. Tag order is noise for
/// the same reason.
#[test]
fn key_and_tag_order_do_not_change_the_input() {
    let ordered = record(
        json!({ "alpha": "first", "beta": "second" }),
        &["core", "must"],
    );
    let shuffled = record(
        json!({ "beta": "second", "alpha": "first" }),
        &["must", "core", "must"],
    );

    let left =
        canonical(&ordered, &"a".repeat(64), crate::vector::DEFAULT_MAX_CHARS).expect("canonical");
    let right =
        canonical(&shuffled, &"a".repeat(64), crate::vector::DEFAULT_MAX_CHARS).expect("canonical");

    assert_eq!(left.input_sha256, right.input_sha256);
    assert_eq!(left.text, right.text);
}

/// Provenance is deliberately absent: a re-import or a compaction rewrites who
/// wrote a record and when, while the meaning it carries is unchanged. If those
/// fields fed the hash, every such rewrite would re-embed the whole corpus.
#[test]
fn provenance_never_reaches_the_embedding_input() {
    let plain = record(json!({ "rule": "Run checks." }), &[]);
    let mut annotated = record(json!({ "rule": "Run checks." }), &[]);
    annotated.id = Uuid::now_v7();
    annotated.actor = "another-agent".into();
    annotated.recorded_at = "2030-06-06T06:06:06Z".into();
    annotated.observed_at = "2030-06-06T06:06:06Z".into();
    annotated.valid_at = "2030-06-06T06:06:06Z".into();
    annotated.supersedes = Some(Uuid::now_v7());
    annotated.evidence = vec![EvidenceRef {
        kind: "legacy.actor".into(),
        reference: "someone".into(),
        sha256: None,
    }];

    let left =
        canonical(&plain, &"a".repeat(64), crate::vector::DEFAULT_MAX_CHARS).expect("canonical");
    let right = canonical(
        &annotated,
        &"b".repeat(64),
        crate::vector::DEFAULT_MAX_CHARS,
    )
    .expect("canonical");

    assert_eq!(left.input_sha256, right.input_sha256);
    assert!(!left.text.contains("legacy.actor"));
}

/// Meaning does change the hash: a different value, a different path, or a
/// different namespace or type must all produce a different embedding input.
#[test]
fn payload_path_value_and_coordinates_change_the_input() {
    let base = canonical(
        &record(json!({ "rule": "Run checks." }), &[]),
        &"a".repeat(64),
        crate::vector::DEFAULT_MAX_CHARS,
    )
    .expect("canonical")
    .input_sha256;
    let other_value = canonical(
        &record(json!({ "rule": "Skip checks." }), &[]),
        &"a".repeat(64),
        crate::vector::DEFAULT_MAX_CHARS,
    )
    .expect("canonical")
    .input_sha256;
    let other_path = canonical(
        &record(json!({ "note": "Run checks." }), &[]),
        &"a".repeat(64),
        crate::vector::DEFAULT_MAX_CHARS,
    )
    .expect("canonical")
    .input_sha256;
    let mut moved = record(json!({ "rule": "Run checks." }), &[]);
    moved.type_name = "agent.lesson.v2".into();
    let other_type = canonical(&moved, &"a".repeat(64), crate::vector::DEFAULT_MAX_CHARS)
        .expect("canonical")
        .input_sha256;
    let mut tagged = record(json!({ "rule": "Run checks." }), &["core"]);
    tagged.namespace = "agent.memory".into();
    let tagged = canonical(&tagged, &"a".repeat(64), crate::vector::DEFAULT_MAX_CHARS)
        .expect("canonical")
        .input_sha256;

    for other in [other_value, other_path, other_type, tagged] {
        assert_ne!(base, other);
    }
}

#[test]
fn a_record_without_embeddable_content_is_refused() {
    let empty = record(json!({}), &[]);
    let mut blank = record(json!({}), &[]);
    blank.namespace = " ".into();
    blank.type_name = " ".into();

    assert!(canonical(&empty, &"a".repeat(64), crate::vector::DEFAULT_MAX_CHARS).is_ok());
    assert!(canonical(&blank, &"a".repeat(64), crate::vector::DEFAULT_MAX_CHARS).is_err());
}

fn record(payload: Value, tags: &[&str]) -> StoredRecord {
    StoredRecord {
        id: Uuid::now_v7(),
        namespace: "agent.memory".into(),
        type_name: "agent.lesson.v1".into(),
        actor: "owner".into(),
        recorded_at: "2026-01-01T00:00:00Z".into(),
        observed_at: "2026-01-01T00:00:00Z".into(),
        valid_at: "2026-01-01T00:00:00Z".into(),
        payload,
        evidence: Vec::new(),
        tags: tags.iter().map(|tag| (*tag).into()).collect(),
        supersedes: None,
    }
}

/// The cap is applied before the digest, and that order is the contract.
///
/// The digest is what the delta pass compares to decide whether a record needs
/// re-embedding. If it described the whole record while the vector was computed
/// from the first 2000 characters, a change to the cap would leave every long
/// record looking already done — and the store would mix vectors built from
/// different preprocessing with nothing saying so.
#[test]
fn the_input_digest_describes_the_capped_text_not_the_whole_record() {
    let long = record(json!({ "rule": "x".repeat(5_000) }), &[]);

    let capped = canonical(&long, &"a".repeat(64), 2_000).expect("canonical");
    let wider = canonical(&long, &"a".repeat(64), 4_000).expect("canonical");

    assert_eq!(
        capped.text.chars().count(),
        2_000,
        "the input was not capped"
    );
    assert_eq!(
        capped.input_sha256,
        crate::kernel::digest::sha256_hex(capped.text.as_bytes()),
        "the digest is not the digest of what would be sent"
    );
    assert_ne!(
        capped.input_sha256, wider.input_sha256,
        "two caps produced the same digest, so a cap change would be invisible"
    );
    // And the record itself is untouched: this shortens an input, never a
    // record. The ledger payload is still the full five thousand characters.
    assert_eq!(
        long.payload["rule"].as_str().expect("rule").chars().count(),
        5_000
    );
}

/// A record shorter than the cap is not touched at all, which is what lets a
/// store adopt the default without re-embedding anything.
#[test]
fn a_record_under_the_cap_keeps_the_digest_it_already_had() {
    let short = record(json!({ "rule": "Run the checks." }), &[]);

    let capped = canonical(&short, &"a".repeat(64), 2_000).expect("canonical");
    let uncapped = canonical(&short, &"a".repeat(64), usize::MAX).expect("canonical");

    assert_eq!(capped.input_sha256, uncapped.input_sha256);
}

/// Unicode is counted in scalar values. A byte cap would cut a multi-byte
/// character in half and produce text that is not valid UTF-8.
#[test]
fn the_document_cap_counts_characters_and_never_splits_one() {
    for character in ['a', 'я', '🦀'] {
        let payload = character.to_string().repeat(3_000);
        let long = record(json!({ "rule": payload }), &[]);

        let capped = canonical(&long, &"a".repeat(64), 2_000).expect("canonical");

        assert_eq!(capped.text.chars().count(), 2_000, "{character}");
        assert!(capped.text.is_char_boundary(capped.text.len()));
    }
}
