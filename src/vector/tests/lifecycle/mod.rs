//! Lifecycle on the read path: answered from the projection, and answering the
//! same thing the ledger did.
//!
//! A semantic page has to skip records a later one replaced and the tombstones
//! that withdrew them. That used to mean reading every record in the store,
//! twice, on every call. It is now two indexed lookups, so these tests carry
//! the burden the ledger walk used to carry implicitly: that the answers are
//! identical, and that no record is read to produce them.
mod equivalence;
mod reads;

use super::support;
use crate::command::init;
use crate::projection::SearchRequest;
use crate::record::{REVOKED_TAG, RecordDraft, StoredRecord, append, read_all, revoke};
use crate::schema::{self, TypeDefinition};
use serde_json::json;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// The ledger's own answer to "which of these is history", written out in full.
///
/// This is the code the read path used to run. Keeping it here as a reference
/// is the whole point of an equivalence fixture: the projection is only allowed
/// to be faster, not different.
fn history_by_ledger(root: &Path) -> HashSet<Uuid> {
    let records = read_all(root).expect("ledger");
    let replaced = records
        .iter()
        .filter_map(|record| record.supersedes)
        .collect::<HashSet<_>>();
    records
        .iter()
        .filter(|record| replaced.contains(&record.id) || withdrawn(record))
        .map(|record| record.id)
        .collect()
}

fn withdrawn(record: &StoredRecord) -> bool {
    record
        .tags
        .iter()
        .any(|tag| tag == REVOKED_TAG || tag == "status:revoked")
}

/// Current, superseded, revoked through the writer, revoked under the legacy
/// tag, and a second type so a scoped question has something to leave out.
fn populated(name: &str) -> PathBuf {
    let root = store(name);
    add(
        &root,
        "agent.lesson.v1",
        "a rule that still stands",
        &[],
        None,
    );
    let replaced = add(&root, "agent.lesson.v1", "a rule that moved on", &[], None);
    add(
        &root,
        "agent.lesson.v1",
        "the rule that replaced it",
        &[],
        Some(replaced),
    );
    let withdrawn = add(&root, "agent.lesson.v1", "a rule taken back", &[], None);
    revoke(&root, withdrawn, Some("no longer true"), "owner").expect("revoke");
    add(
        &root,
        "agent.lesson.v1",
        "a rule withdrawn the old way",
        &["status:revoked"],
        None,
    );
    add(&root, "agent.note.v1", "a note in another type", &[], None);
    // The text index is caught up after confirmation, not inside it, so a
    // fixture that asks the projection a question immediately would be asking
    // it about records it has not been handed yet. Driving the catch-up here
    // makes these tests about what the projection ANSWERS rather than about
    // when it was told.
    crate::projection::catch_up_text(&root).expect("catch up the text index");
    root
}

fn request(limit: u16) -> SearchRequest {
    SearchRequest {
        query: Some("rule".into()),
        namespace: Some("agent.memory".into()),
        type_name: Some("agent.lesson.v1".into()),
        limit,
    }
}

fn store(name: &str) -> PathBuf {
    let root = support::root(name);
    init::create(&root, "owner", "agent.memory").expect("initialize");
    for type_name in ["agent.lesson.v1", "agent.note.v1"] {
        schema::register(
            &root,
            TypeDefinition {
                type_name: type_name.into(),
                uri: format!("equill://{}/v1", type_name.trim_end_matches(".v1")),
                owner: "owner".into(),
                payload_schema: json!({
                    "$schema": "https://json-schema.org/draft/2020-12/schema",
                    "type": "object",
                    "properties": { "rule": { "type": "string" } },
                    "required": ["rule"],
                    "additionalProperties": false
                }),
                lifecycle: Default::default(),
            },
            "owner",
        )
        .expect("register schema");
    }
    root
}

fn add(root: &Path, type_name: &str, rule: &str, tags: &[&str], supersedes: Option<Uuid>) -> Uuid {
    append(
        root,
        RecordDraft {
            namespace: "agent.memory".into(),
            type_name: type_name.into(),
            observed_at: "2026-01-01T00:00:00Z".into(),
            valid_at: None,
            payload: json!({ "rule": rule }),
            evidence: Vec::new(),
            tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
            supersedes,
        },
        "owner",
    )
    .expect("append")
    .id
}
