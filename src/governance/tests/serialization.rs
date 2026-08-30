use super::super::{grant, transfer};
use super::store;
use crate::kernel::error::Error;
use crate::record::{RecordDraft, append_only, require_current_writer};
use crate::schema::{self, TypeDefinition};
use serde_json::json;
use std::path::Path;

/// The race the writer used to lose: authority was read before any lock was
/// taken, so an actor could pass that check, wait for the lock, and append after
/// a handover had already removed them.
///
/// This is the check that closes it, exercised directly. It is the step that
/// runs while the writer lock is held, and it must answer from the store as it
/// is at that moment — not from the config the caller read on the way in.
#[test]
fn the_under_lock_check_answers_from_the_store_as_it_is_now() {
    let root = store();
    register(&root);
    let stale = crate::kernel::store::load(&root).expect("load");

    require_current_writer(&root, "founder", "agent.memory", "agent.lesson.v1")
        .expect("the owner may append before the handover");
    transfer(&root, "successor", None, "founder").expect("handover");

    // The caller still holds a config in which founder is the owner. The
    // re-check must not agree with it.
    crate::kernel::identity::require_type_writer(
        &stale,
        "founder",
        "agent.memory",
        "agent.lesson.v1",
    )
    .expect("the stale config still says yes, which is the whole problem");
    assert!(matches!(
        require_current_writer(&root, "founder", "agent.memory", "agent.lesson.v1"),
        Err(Error::PermissionDenied)
    ));
}

/// End to end: the actor who lost access cannot append, and the one whose scoped
/// grant survived still can. Serialization is what makes both answers stable
/// rather than dependent on who reached the lock first.
#[test]
fn a_handover_decides_every_append_that_follows_it() {
    let root = store();
    register(&root);
    grant(
        &root,
        "worker",
        "agent.memory",
        &["agent.lesson.v1".into()],
        None,
        "founder",
    )
    .expect("grant");

    append(&root, "founder").expect("the owner appends before");
    transfer(&root, "successor", None, "founder").expect("handover");

    assert!(matches!(
        append(&root, "founder"),
        Err(Error::PermissionDenied)
    ));
    append(&root, "worker").expect("an untouched scoped grant keeps working");
    append(&root, "successor").expect("the new owner appends");
}

fn register(root: &Path) {
    schema::register(
        root,
        TypeDefinition {
            type_name: "agent.lesson.v1".into(),
            uri: "equill://agent.lesson/v1".into(),
            owner: "founder".into(),
            payload_schema: json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": { "rule": { "type": "string" } },
                "required": ["rule"],
                "additionalProperties": false
            }),
            lifecycle: Default::default(),
        },
        "founder",
    )
    .expect("register");
}

fn append(root: &Path, actor: &str) -> Result<crate::record::AppendReport, Error> {
    append_only(
        root,
        RecordDraft {
            namespace: "agent.memory".into(),
            type_name: "agent.lesson.v1".into(),
            observed_at: "2026-01-01T00:00:00Z".into(),
            valid_at: None,
            payload: json!({ "rule": "synthetic" }),
            evidence: Vec::new(),
            tags: Vec::new(),
            supersedes: None,
        },
        actor,
    )
}
