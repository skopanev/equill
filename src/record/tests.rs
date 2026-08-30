use super::{RecordDraft, append};
use crate::command::init;
use crate::projection::{self, ProjectionState, SearchRequest};
use crate::schema::{self, TypeDefinition};
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);

fn store() -> PathBuf {
    let suffix = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("equill-record-{}-{suffix}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    init::create(&path, "writer", "agent.memory").expect("initialize");
    schema::register(
        &path,
        TypeDefinition {
            type_name: "agent.lesson.v1".into(),
            uri: "equill://agent.lesson/v1".into(),
            owner: "schema-owner".into(),
            payload_schema: json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": { "rule": { "type": "string" } },
                "required": ["rule"],
                "additionalProperties": false
            }),
            lifecycle: Default::default(),
        },
        "writer",
    )
    .expect("register schema");
    path
}

fn draft(payload: serde_json::Value) -> RecordDraft {
    RecordDraft {
        namespace: "agent.memory".into(),
        type_name: "agent.lesson.v1".into(),
        observed_at: "2026-01-01T12:00:00Z".into(),
        valid_at: None,
        payload,
        evidence: Vec::new(),
        tags: vec!["testing".into()],
        supersedes: None,
    }
}

#[test]
fn appends_valid_record_without_payload_in_receipt() {
    let path = store();
    let report =
        append(&path, draft(json!({ "rule": "Run checks." })), "writer").expect("append record");
    let contents = fs::read_to_string(path.join(&report.ledger)).expect("read ledger");
    let scan = crate::integrity::scan(&path).expect("full integrity scan");
    let search = projection::search(
        &path,
        &SearchRequest {
            query: Some("Run checks".into()),
            namespace: Some("agent.memory".into()),
            type_name: Some("agent.lesson.v1".into()),
            limit: 10,
        },
    )
    .expect("search projection");

    assert_eq!(contents.lines().count(), 1);
    assert_eq!(scan.records, 1);
    assert_eq!(report.projection, ProjectionState::Ready);
    assert_eq!(search.hits.len(), 1);
    assert!(
        !serde_json::to_string(&report)
            .expect("report")
            .contains("Run checks")
    );
    let rebuilt = projection::rebuild(&path).expect("rebuild projection");
    assert_eq!(rebuilt.records, 1);
    fs::remove_dir_all(path).expect("remove test store");
}

#[test]
fn rejects_invalid_payload_and_actor() {
    let path = store();
    let payload =
        append(&path, draft(json!({ "rule": 42 })), "writer").expect_err("reject payload");
    let actor = append(&path, draft(json!({ "rule": "safe" })), "guest").expect_err("reject actor");

    assert!(payload.to_string().contains("does not match"));
    assert!(actor.to_string().contains("not allowed"));
    fs::remove_dir_all(path).expect("remove test store");
}

#[test]
fn invalid_payload_names_the_field_and_the_constraint() {
    // The author of a lesson appends JSONL by hand and cannot see the registered
    // contract. "does not match" alone sends them hunting through the schema.
    let suffix = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "equill-record-detail-{}-{suffix}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    init::create(&path, "writer", "agent.memory").expect("initialize");
    schema::register(
        &path,
        TypeDefinition {
            type_name: "agent.lesson.v1".into(),
            uri: "equill://agent.lesson/v1".into(),
            owner: "schema-owner".into(),
            payload_schema: json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": {
                    "rule": { "type": "string", "maxLength": 500 },
                    "source": { "type": "string", "enum": ["gate", "panel", "owner"] }
                },
                "required": ["rule", "source"],
                "additionalProperties": false
            }),
            lifecycle: Default::default(),
        },
        "writer",
    )
    .expect("register schema");

    let long_rule = "x".repeat(1070);
    let error = append(
        &path,
        draft(json!({ "rule": long_rule, "source": "gm" })),
        "writer",
    )
    .expect_err("reject payload");
    let message = error.to_string();

    assert!(message.contains("/rule"), "{message}");
    assert!(message.contains("longer than 500"), "{message}");
    assert!(message.contains("/source"), "{message}");
    assert!(message.contains("\"gm\""), "{message}");
    // The offending value must not bury the reason that follows it.
    assert!(message.len() < 600, "message is {} bytes", message.len());

    let missing = append(
        &path,
        draft(json!({ "rule": "long enough rule" })),
        "writer",
    )
    .expect_err("reject missing field");
    assert!(
        missing
            .to_string()
            .contains("\"source\" is a required property"),
        "{missing}"
    );

    fs::remove_dir_all(path).expect("remove test store");
}

/// A write the defense refused never reaches the ledger, so there is nothing for
/// the index to catch up on. Reporting it as queued would describe work that
/// will never happen for a record that does not exist.
#[test]
fn a_blocked_write_reports_no_projection_state_at_all() {
    let root = store();
    let refused = append(
        &root,
        RecordDraft {
            namespace: "agent.memory".into(),
            type_name: "agent.lesson.v1".into(),
            observed_at: "2026-01-01T00:00:00Z".into(),
            valid_at: None,
            // A synthetic credential shape, present only to trip the scanner.
            payload: serde_json::json!({
                "rule": "AKIAIOSFODNN7EXAMPLE is a key that must never be stored"
            }),
            evidence: Vec::new(),
            tags: Vec::new(),
            supersedes: None,
        },
        "writer",
    );

    assert!(refused.is_err(), "the defense must refuse this write");
    let receipt = latest_receipt(&root);
    assert_eq!(receipt["status"], "blocked-by-memory-defense");
    assert_eq!(receipt["durable"], false, "nothing was made durable");
    assert_eq!(
        receipt["projection"], "not-applicable",
        "a record that does not exist has no projection state"
    );
    let _ = std::fs::remove_dir_all(&root);
}

fn latest_receipt(root: &std::path::Path) -> serde_json::Value {
    let mut found: Vec<std::path::PathBuf> = Vec::new();
    for month in std::fs::read_dir(root.join("receipts/writes")).expect("receipts") {
        for entry in std::fs::read_dir(month.expect("month").path()).expect("month entries") {
            found.push(entry.expect("receipt").path());
        }
    }
    found.sort();
    serde_json::from_slice(&std::fs::read(found.last().expect("a receipt")).expect("read"))
        .expect("json")
}

/// The confirmation boundary, observed rather than timed.
///
/// A caller is told a record is durable once the ledger holds it and its
/// receipt is committed. Nothing before that point may scan the ledger, rebuild
/// the lifecycle graph, or open a projection transaction: those are rebuildable
/// work, and a write that waits for them is paying for history it already has.
///
/// The end-to-end benchmark measures the consequence — confirmation not getting
/// slower as a store grows. This measures the cause, so a slow machine cannot
/// hide it and a fast one cannot excuse it.
#[test]
fn confirmation_touches_no_rebuildable_work() {
    let root = store();
    // A store with some history, so a scan would have something to find.
    for index in 0..20 {
        append(&root, lesson(&format!("existing lesson {index}")), "writer").expect("seed");
    }

    super::hotpath::reset();
    append(&root, lesson("the record under test"), "writer").expect("append");
    let touched = super::hotpath::touched();

    assert_eq!(
        touched,
        super::hotpath::Touched::default(),
        "confirmation did rebuildable work: {touched:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

fn lesson(rule: &str) -> RecordDraft {
    RecordDraft {
        namespace: "agent.memory".into(),
        type_name: "agent.lesson.v1".into(),
        observed_at: "2026-01-01T00:00:00Z".into(),
        valid_at: None,
        payload: json!({ "rule": rule }),
        evidence: Vec::new(),
        tags: Vec::new(),
        supersedes: None,
    }
}
