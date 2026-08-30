use super::{REVOKED_TAG, revoke};
use crate::command::init;
use crate::record::{RecordDraft, append_indexed, read_all};
use crate::schema::{self, LifecycleMode, LifecyclePolicy, TypeDefinition};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub(super) fn store(name: &str, lifecycle: LifecyclePolicy) -> PathBuf {
    let root = std::env::temp_dir().join(format!("equill-revoke-{name}-{}", Uuid::now_v7()));
    init::create(&root, "owner", "agent.memory").expect("initialize");
    schema::register(
        &root,
        TypeDefinition {
            type_name: "agent.lesson.v1".into(),
            uri: "equill://agent.lesson/v1".into(),
            owner: "owner".into(),
            payload_schema: json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": { "rule": { "type": "string" } },
                "required": ["rule"],
                "additionalProperties": false
            }),
            lifecycle,
        },
        "owner",
    )
    .expect("register schema");
    root
}

pub(super) fn add(root: &Path, rule: &str) -> Uuid {
    append_indexed(
        root,
        RecordDraft {
            namespace: "agent.memory".into(),
            type_name: "agent.lesson.v1".into(),
            observed_at: "2026-01-01T00:00:00Z".into(),
            valid_at: None,
            payload: json!({ "rule": rule }),
            evidence: Vec::new(),
            tags: vec!["core".into()],
            supersedes: None,
        },
        "owner",
    )
    .expect("append")
    .id
}

/// A retraction says "no longer", not "instead": the tombstone repeats the
/// author's own claim rather than inventing a replacement, and the reason
/// lives as evidence so the declared payload shape is untouched.
#[test]
fn a_tombstone_repeats_the_claim_and_keeps_the_reason_out_of_the_payload() {
    let root = store("happy", LifecyclePolicy::default());
    let target = add(&root, "Run the build checks");

    let report = revoke(&root, target, Some("  superseded by policy  "), "owner").expect("revoke");
    let plain = add(&root, "Another rule");
    let silent = revoke(&root, plain, None, "owner").expect("revoke without a reason");

    let records = read_all(&root).expect("records");
    let stone = records
        .iter()
        .find(|record| record.id == report.tombstone)
        .expect("tombstone stored");
    assert_eq!(stone.supersedes, Some(target));
    assert_eq!(stone.payload, json!({ "rule": "Run the build checks" }));
    assert!(stone.tags.iter().any(|tag| tag == REVOKED_TAG));
    // The original tags survive: a retraction does not reclassify the record.
    assert!(stone.tags.iter().any(|tag| tag == "core"));
    let reason = stone
        .evidence
        .iter()
        .find(|item| item.kind == "equill.revocation.comment")
        .expect("reason kept as evidence");
    assert_eq!(reason.reference, "superseded by policy");
    let quiet = records
        .iter()
        .find(|record| record.id == silent.tombstone)
        .expect("second tombstone");
    assert!(quiet.evidence.is_empty(), "no reason means no evidence");
    // Nothing is deleted, and the report names coordinates only.
    assert_eq!(records.len(), 4);
    let json = serde_json::to_string(&report).expect("report");
    assert!(!json.contains("Run the build checks"), "{json}");
    fs::remove_dir_all(root).expect("cleanup");
}

/// Every rule that governs an append governs a revocation, because it is one.
#[test]
fn the_writer_refuses_a_revocation_it_would_refuse_as_an_append() {
    let root = store("denied", LifecyclePolicy::default());
    let target = add(&root, "Run the build checks");
    revoke(&root, target, None, "owner").expect("first revocation");

    let stale = revoke(&root, target, None, "owner").expect_err("already superseded");
    let stranger =
        revoke(&root, add(&root, "Another"), None, "stranger").expect_err("unauthorized actor");
    let missing = revoke(&root, Uuid::now_v7(), None, "owner").expect_err("unknown id");

    assert!(stale.to_string().contains("already superseded"), "{stale}");
    assert!(
        stranger.to_string().contains("not allowed to write"),
        "{stranger}"
    );
    assert!(
        missing.to_string().contains("no record with id"),
        "{missing}"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

/// A type that declared itself append_only means it: there is no back door
/// through revoke.
#[test]
fn an_append_only_type_cannot_be_revoked() {
    let root = store(
        "append-only",
        LifecyclePolicy {
            mode: LifecycleMode::AppendOnly,
            ..LifecyclePolicy::default()
        },
    );
    let target = add(&root, "An event that happened");

    let refused = revoke(&root, target, None, "owner").expect_err("append_only");

    assert!(refused.to_string().contains("append_only"), "{refused}");
    assert_eq!(read_all(&root).expect("records").len(), 1);
    fs::remove_dir_all(root).expect("cleanup");
}

/// A disappearance has to be explainable. An ordinary read shows neither the
/// withdrawn claim nor its tombstone; a caller who asks for the chain gets the
/// tombstone back, and with it the reason its author gave.
#[test]
fn the_chain_read_explains_a_disappearance() {
    use crate::context::{assemble, inline_request, register_profile, register_selector};
    use crate::filter::Filter;

    let root = store("visible", LifecyclePolicy::default());
    let target = add(&root, "Run the build checks");
    let selector = root.join("selector.json");
    fs::write(
        &selector,
        serde_json::to_vec(&json!({
            "id": "lesson.selector", "version": "1",
            "type": "agent.lesson.v1", "strategies": ["exact"]
        }))
        .expect("selector json"),
    )
    .expect("selector file");
    register_selector(&root, &selector, "owner").expect("selector");
    let profile = root.join("profile.json");
    fs::write(
        &profile,
        serde_json::to_vec(&json!({
            "id": "worker.v1", "version": "1", "actors": [],
            "grants": [{ "namespace": "agent.memory", "types": ["agent.lesson.v1"] }],
            "selectors": ["lesson.selector"]
        }))
        .expect("profile json"),
    )
    .expect("profile file");
    register_profile(&root, &profile, "owner").expect("profile");
    let report = revoke(&root, target, Some("no longer true"), "owner").expect("revoke");

    let ask = |chain: bool| {
        let request = inline_request(
            Some("build checks".into()),
            vec![],
            vec![],
            vec![],
            None,
            chain,
        )
        .expect("request");
        assemble(&root, "worker.v1", request, "owner", &Filter::default())
            .expect("context")
            .selected_record_ids
    };
    let ordinary = ask(false);
    let chain = ask(true);

    assert!(!ordinary.contains(&target), "the withdrawn claim is gone");
    assert!(
        !ordinary.contains(&report.tombstone),
        "and so is its tombstone"
    );
    assert!(
        chain.contains(&report.tombstone),
        "the chain explains the absence"
    );
    // The reason travels with the tombstone, so a reader can see why.
    let stone = read_all(&root)
        .expect("records")
        .into_iter()
        .find(|record| record.id == report.tombstone)
        .expect("tombstone");
    assert!(
        stone
            .evidence
            .iter()
            .any(|item| item.reference == "no longer true")
    );
    fs::remove_dir_all(root).expect("cleanup");
}
