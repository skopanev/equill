//! The shape a reader is answered in: which block comes first, which field
//! leads a step, and what a record with no shape at all still reads as.
use super::tests::typed;
use super::{Format, records};
use serde_json::json;

/// The order a reader needs, which is not the order the store gives back: a
/// payload's keys are sorted when they are stored, so `role` arrives after
/// `process` and reading the order off the payload would print the alphabet.
#[test]
fn the_named_three_print_in_the_readers_order_not_the_stores() {
    let printed = records(
        &[typed(
            "agent.role.v1",
            json!({ "process": "Pre-merge audit", "role": "Release reviewer", "steps": ["Read it"] }),
        )],
        Format::Text,
        &[],
    )
    .expect("text");
    let role = printed.find("Role:").expect("role heading");
    let process = printed.find("Process:").expect("process heading");
    let steps = printed.find("Steps:").expect("steps heading");

    assert!(
        role < process,
        "alphabetical order reached the reader: {printed}"
    );
    assert!(process < steps, "steps came before the process: {printed}");
}

/// Steps that arrive as records of their own are ordered by what they say their
/// order is, and numbered by it. A gap is information — step 3 missing between
/// 2 and 4 is a question for the reader, not something to renumber away.
#[test]
fn separate_step_records_are_ordered_and_numbered_by_their_own_number() {
    let printed = records(
        &[
            typed(
                "agent.step.v2",
                json!({ "step": 4, "does": "Run the gates" }),
            ),
            typed(
                "agent.step.v2",
                json!({ "step": 2, "does": "Read the diff", "on_fail": "Stop" }),
            ),
        ],
        Format::Text,
        &[],
    )
    .expect("text");

    assert_eq!(
        printed, "Steps:\n2. Do: Read the diff\n   On fail: Stop\n4. Do: Run the gates",
        "the ledger's order or a renumbering reached the reader"
    );
}

/// A step without a gate has no gate, and an empty label would say it has one
/// that is blank. The absent parts are absent, not empty.
#[test]
fn a_step_without_a_gate_prints_no_gate_label() {
    let printed = records(
        &[typed(
            "agent.step.v2",
            json!({ "step": 1, "does": "Read the diff" }),
        )],
        Format::Text,
        &[],
    )
    .expect("text");

    assert_eq!(printed, "Steps:\n1. Do: Read the diff");
}

/// A record with none of the three shapes still has to read as something. The
/// fallback prints what the record carries under its own names — it does not
/// invent a Role for a record that never claimed one.
#[test]
fn a_record_with_no_shape_reads_as_labels_and_invents_nothing() {
    let printed = records(
        &[typed(
            "agent.note.v1",
            json!({ "text": "A record with no role and no steps.", "author": "auditor" }),
        )],
        Format::Text,
        &[],
    )
    .expect("text");

    assert!(printed.contains("Author: auditor"), "{printed}");
    assert!(printed.contains("Text: A record with no role"), "{printed}");
    assert!(
        !printed.contains("Role:") && !printed.contains("Steps:"),
        "headings were invented for a record that has neither: {printed}"
    );
}

/// The reader's order, from records that arrive in the wrong one.
///
/// Fed in the right order this proves nothing: the correct output is produced
/// by grouping and equally by not grouping, so the assertion would hold over a
/// renderer that simply printed what it was given. The step is deliberately
/// first, the role last.
#[test]
fn the_blocks_group_into_the_readers_order_whatever_order_they_arrive_in() {
    let printed = records(
        &[
            typed(
                "agent.step.v2",
                json!({ "step": 1, "does": "Read the diff" }),
            ),
            typed(
                "agent.process.v2",
                json!({ "title": "Pre-merge audit", "purpose": "Catch defects early" }),
            ),
            typed(
                "agent.role.v2",
                json!({ "role": "Release reviewer", "do": "Review", "kind": "human" }),
            ),
        ],
        Format::Text,
        &[],
    )
    .expect("text");
    let role = printed.find("Role:").expect("role");
    let process = printed.find("Process:").expect("process");
    let steps = printed.find("Steps:").expect("steps");

    assert!(
        role < process && process < steps,
        "the answer printed the order the records arrived in:\n{printed}"
    );
}

/// A step is read for what it says to do. Bookkeeping about the step —  who
/// owns it, which project it belongs to — is printed after, because a reader
/// scanning for the instruction should not have to step over the metadata.
#[test]
fn a_step_leads_with_what_to_do_not_with_its_bookkeeping() {
    let separate = records(
        &[typed(
            "agent.step.v2",
            json!({ "actor": "reviewer", "does": "Read the diff", "gate": "Opened", "on_fail": "Ask" }),
        )],
        Format::Text,
        &[],
    )
    .expect("text");
    // The same step written inside one record rather than as a record of its
    // own: two code paths, one reader, so both are asserted here — fixing one
    // and leaving the other is exactly the failure this catches.
    let inline = records(
        &[typed(
            "agent.role.v1",
            json!({
                "role": "Reviewer",
                "steps": [{ "actor": "reviewer", "does": "Read the diff", "gate": "Opened" }]
            }),
        )],
        Format::Text,
        &[],
    )
    .expect("text");

    for printed in [&separate, &inline] {
        let doing = printed.find("Do: ").expect("do");
        let actor = printed.find("Actor: ").expect("actor");
        assert!(
            doing < actor,
            "the step's bookkeeping came before the instruction:\n{printed}"
        );
    }
    assert!(
        separate.find("Gate:").expect("gate") < separate.find("On fail:").expect("on fail"),
        "gate and on-fail are not in the order they are read in:\n{separate}"
    );
}

/// A process is named by its title; its purpose is what it is for, and belongs
/// in the body under its own label rather than standing in for the name.
#[test]
fn a_process_is_headed_by_its_title_and_keeps_its_purpose_below() {
    let printed = records(
        &[typed(
            "agent.process.v2",
            json!({ "title": "Pre-merge audit", "name": "premerge", "purpose": "Catch defects early" }),
        )],
        Format::Text,
        &[],
    )
    .expect("text");

    assert!(
        printed.starts_with("Process: Pre-merge audit"),
        "the process is not headed by its title:\n{printed}"
    );
    assert!(
        printed.contains("Purpose: Catch defects early"),
        "the purpose lost its label:\n{printed}"
    );
}
