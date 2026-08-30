//! What the text catch-up publishes, and what it refuses to publish.
use crate::command::init;
use crate::record::{RecordDraft, append_only};
use crate::schema::{self, TypeDefinition};
use serde_json::json;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// A record the index cannot take must be retried, not skipped.
///
/// The cursor into the ledger is a count, which makes stepping over a failed
/// record permanent: every later pass starts after it, and only a full rebuild
/// would ever come back. So a pass that did not index everything publishes
/// nothing — it leaves the previous position standing, which costs a repeat of
/// the records that did succeed and buys the retry.
///
/// The consequence for readers is the point: while the record is missing, the
/// published position says the index is behind. It never says current.
#[test]
fn a_record_the_index_refuses_is_retried_and_never_reported_as_covered() {
    let root = store();
    for index in 0..3 {
        append_only(&root, lesson(&format!("lesson {index}")), "writer").expect("seed");
    }
    super::catch_up_text(&root).expect("first pass");
    let covered = super::watermark(&root).expect("a published position");
    assert_eq!(
        covered.indexed_records, 3,
        "the first pass covered the store"
    );

    append_only(&root, lesson("the record the index will refuse"), "writer").expect("append");
    let database = root.join("projections/sqlite/equill.sqlite3");
    seal(&database);
    let indexed = super::catch_up_text(&root).expect("pass against a sealed index");
    unseal(&database);

    assert_eq!(indexed, 0, "the sealed index accepted a record");
    assert_eq!(
        super::state(&root).expect("state"),
        super::ProjectionState::Degraded,
        "a record the index refused left the projection reported as healthy"
    );
    let after = super::watermark(&root).expect("the previous position still stands");
    assert_eq!(
        after.indexed_records, 3,
        "a pass that indexed nothing published coverage it did not have"
    );
    assert_eq!(
        after.ledger_bytes, covered.ledger_bytes,
        "a failed pass moved the published ledger position"
    );
    // Which is what makes the retry possible at all.
    let retried = super::catch_up_text(&root).expect("retry");
    assert_eq!(retried, 1, "the retry did not reach the refused record");
    assert_eq!(
        super::watermark(&root).expect("position").indexed_records,
        4,
        "a completed pass did not publish its coverage"
    );

    // Health follows coverage: the record that was refused is in the index now,
    // so the marker that recorded its refusal has nothing left to describe.
    assert_eq!(
        super::state(&root).expect("state"),
        super::ProjectionState::Ready,
        "a store that caught up completely is still reported as degraded"
    );

    let found = super::search(
        &root,
        &super::SearchRequest {
            query: Some("refuse".into()),
            namespace: Some("agent.memory".into()),
            type_name: Some("agent.lesson.v1".into()),
            limit: 10,
        },
    )
    .expect("search");
    assert_eq!(
        found.hits.len(),
        1,
        "the retried record is still not searchable"
    );
    let _ = fs::remove_dir_all(&root);
}

/// A month boundary must not lose the records on either side of it.
///
/// The cursor into the ledger is a count, which is only sound if the sequence
/// it counts into grows at the end. Two things make that true: the writer names
/// each ledger by the month it is writing, so a new file is always the latest,
/// and `read_all` sorts the ledgers rather than taking whatever order the
/// directory hands back. Without the sort the new month can arrive anywhere in
/// the sequence, and every record the cursor steps over is skipped for good —
/// a fault no store shows until it has lived through the turn of a month.
#[test]
fn a_record_in_a_new_ledger_is_covered_rather_than_stepped_over() {
    let root = store();
    for index in 0..3 {
        append_only(&root, lesson(&format!("lesson {index}")), "writer").expect("seed");
    }
    super::catch_up_text(&root).expect("first pass");

    // The next month, written the way the writer would write it: its own
    // ledger, named for its own month.
    let existing = fs::read_dir(root.join("records"))
        .expect("records")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .next()
        .expect("a ledger");
    let mut record: serde_json::Value = serde_json::from_str(
        fs::read_to_string(&existing)
            .expect("ledger")
            .lines()
            .next()
            .expect("a record"),
    )
    .expect("json");
    let object = record.as_object_mut().expect("object");
    object.insert("id".into(), json!(Uuid::now_v7().to_string()));
    object.insert("recorded_at".into(), json!("2099-01-01T00:00:00Z"));
    object.insert(
        "payload".into(),
        json!({ "rule": "the record in the next ledger" }),
    );
    fs::write(
        root.join("records/2099-01.jsonl"),
        format!("{}\n", serde_json::to_string(&record).expect("line")),
    )
    .expect("next ledger");

    let indexed = super::catch_up_text(&root).expect("second pass");
    assert_eq!(indexed, 1, "the record in the new ledger was not indexed");
    assert_eq!(
        super::watermark(&root).expect("position").indexed_records,
        4,
        "the published coverage does not account for both ledgers"
    );
    let found = super::search(
        &root,
        &super::SearchRequest {
            query: Some("next ledger".into()),
            namespace: Some("agent.memory".into()),
            type_name: Some("agent.lesson.v1".into()),
            limit: 10,
        },
    )
    .expect("search");
    assert_eq!(
        found.hits.len(),
        1,
        "the record in the new ledger is not searchable"
    );
    let _ = fs::remove_dir_all(&root);
}

fn seal(path: &Path) {
    let mut permissions = fs::metadata(path).expect("metadata").permissions();
    permissions.set_mode(0o444);
    fs::set_permissions(path, permissions).expect("seal");
}

fn unseal(path: &Path) {
    let mut permissions = fs::metadata(path).expect("metadata").permissions();
    permissions.set_mode(0o644);
    let _ = fs::set_permissions(path, permissions);
}

fn store() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "equill-catchup-{}-{}",
        std::process::id(),
        Uuid::now_v7()
    ));
    init::create(&root, "writer", "agent.memory").expect("initialize");
    schema::register(
        &root,
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
    root
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
