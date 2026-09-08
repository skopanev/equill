//! Every selected record reaches the answer, or the answer is not the contract.
//!
//! The formatter used to drop whatever it had no shape for, and the receipt
//! stayed green while the agent never saw the record. These assert the absence
//! of that: the loss was silent, so only a check that names each record can
//! find it.
use super::answer;
use crate::record::StoredRecord;
use serde_json::{Value, json};

pub(super) fn record(type_name: &str, payload: Value) -> StoredRecord {
    StoredRecord {
        id: uuid::Uuid::now_v7(),
        namespace: "agent.memory".into(),
        type_name: type_name.into(),
        actor: "owner".into(),
        recorded_at: "2026-01-01T00:00:00Z".into(),
        observed_at: "2026-01-01T00:00:00Z".into(),
        valid_at: "2026-01-01T00:00:00Z".into(),
        payload,
        evidence: Vec::new(),
        tags: Vec::new(),
        supersedes: None,
    }
}

/// A type this formatter knows nothing about is still part of the answer.
#[test]
fn an_unrecognized_type_reaches_the_answer() {
    let out = answer(&[record(
        "agent.project.v1",
        json!({ "project": "sample-alpha", "lane_limit": 4 }),
    )]);

    assert!(out.contains("sample-alpha"), "{out}");
    assert!(out.contains("lane_limit: 4"), "{out}");
}

/// `null`, `false`, zero and empty containers are facts, and a contract that
/// renders them as nothing says something different from what the record says:
/// a limit deliberately unset reads as a limit never mentioned.
#[test]
fn an_explicit_absence_is_not_rendered_as_an_absence() {
    let out = answer(&[record(
        "agent.project.v1",
        json!({
            "project": null,
            "lane_limit": 0,
            "active": false,
            "lanes": [],
            "marker": "sample-beta"
        }),
    )]);

    assert!(
        out.contains("project: null"),
        "explicit null was lost: {out}"
    );
    assert!(out.contains("lane_limit: 0"), "zero was lost: {out}");
    assert!(out.contains("active: false"), "false was lost: {out}");
    assert!(out.contains("lanes: []"), "the empty list was lost: {out}");
}

/// A record recognized correctly and still saying nothing through its section.
///
/// This is the half no check against unknown types can find: the branch was
/// chosen, ran, produced nothing, and the record vanished anyway.
#[test]
fn a_recognized_record_with_nothing_to_render_still_reaches_the_answer() {
    let out = answer(&[
        record(
            "agent.role.v2",
            json!({ "role": "reviewer", "why": "sample-role" }),
        ),
        record("agent.process.v2", json!({ "title": "sample-process" })),
        record(
            "agent.rule.v1",
            json!({ "module": "communication", "key": "sample-rule" }),
        ),
        record("agent.step.v2", json!({ "step": 3, "why": "sample-step" })),
    ]);

    for marker in [
        "sample-role",
        "sample-process",
        "sample-rule",
        "sample-step",
    ] {
        assert!(out.contains(marker), "{marker} was dropped:\n{out}");
    }
}

/// A step is dropped by the renderer when it has no instruction, so counting it
/// as printed at the moment it is queued makes the record vanish one layer
/// down.
#[test]
fn a_step_the_renderer_will_drop_is_covered_elsewhere() {
    let out = answer(&[record(
        "agent.step.v2",
        json!({ "step": 7, "gate": "sample-gate-only" }),
    )]);

    assert!(out.contains("sample-gate-only"), "{out}");
}

/// An unknown record that also carries inline steps keeps its own payload: the
/// steps are content, but they are not the rest of what the record says.
#[test]
fn inline_steps_do_not_stand_in_for_the_records_own_payload() {
    let out = answer(&[record(
        "agent.playbook.v1",
        json!({
            "owner": "sample-owner",
            "steps": [{ "do": "sample-inline-step" }]
        }),
    )]);

    assert!(
        out.contains("sample-inline-step"),
        "the step was lost: {out}"
    );
    assert!(
        out.contains("owner: sample-owner"),
        "the record's own payload was hidden by its steps: {out}"
    );
}

/// A record whose payload is empty, or absent entirely, was still selected.
///
/// "It says nothing" is what this record says, and dropping it is the same
/// silence this fix removes: the answer has to account for what the selection
/// returned, including the record that turned out to hold nothing.
#[test]
fn an_empty_or_null_payload_still_reaches_the_answer() {
    let out = answer(&[
        record("sample.empty.v1", json!({})),
        record("sample.null.v1", Value::Null),
    ]);

    assert!(
        out.contains("sample.empty.v1"),
        "an empty payload vanished: {out}"
    );
    assert!(
        out.contains("sample.null.v1"),
        "a null payload vanished: {out}"
    );
}
