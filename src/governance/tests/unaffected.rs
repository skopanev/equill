use super::super::{show, transfer};
use super::store;
use crate::kernel::error::Error;
use crate::kernel::identity;
use crate::kernel::store as config;
use crate::record::{RecordDraft, append_only};
use crate::schema::{self, TypeDefinition};
use serde_json::json;

/// A scoped writer keeps writing across a handover. Governance changed hands;
/// their least-privilege access did not.
#[test]
fn an_ordinary_scoped_writer_is_unaffected_by_a_handover() {
    let root = store();
    register_lesson(&root, "founder");
    super::super::grant(
        &root,
        "worker",
        "agent.memory",
        &["agent.lesson.v1".into()],
        None,
        "founder",
    )
    .expect("grant");

    append(&root, "worker").expect("the scoped writer appends before the handover");
    transfer(&root, "successor", None, "founder").expect("transfer");
    append(&root, "worker").expect("and still appends after it");

    let after = config::load(&root).expect("load");
    assert!(append(&root, "founder").is_err(), "the old owner cannot");
    identity::require_type_writer(&after, "worker", "agent.memory", "agent.lesson.v1")
        .expect("the grant survived");
    assert_eq!(show(&root).expect("show").grants.len(), 1);
}

fn register_lesson(root: &std::path::Path, actor: &str) {
    schema::register(
        root,
        TypeDefinition {
            type_name: "agent.lesson.v1".into(),
            uri: "equill://agent.lesson/v1".into(),
            owner: actor.into(),
            payload_schema: json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": { "rule": { "type": "string" } },
                "required": ["rule"],
                "additionalProperties": false
            }),
            lifecycle: Default::default(),
        },
        actor,
    )
    .expect("register");
}

fn append(root: &std::path::Path, actor: &str) -> Result<crate::record::AppendReport, Error> {
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
