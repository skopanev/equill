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

/// One line for the role, however many records describe it.
///
/// Repeats collapse, because several records describing one role is a normal
/// thing for a store to hold and a heading each makes one role look like
/// several. Different roles are all named on that line: hiding one would be
/// hiding a fact rather than tidying a heading.
#[test]
fn every_role_is_named_on_exactly_one_line() {
    let printed = records(
        &[
            typed(
                "agent.role.v2",
                json!({ "role": "Reviewer", "do": "Guard the merge", "kind": "human" }),
            ),
            typed("agent.role.v2", json!({ "role": "Reviewer" })),
            typed("agent.role.v2", json!({ "role": "Release captain" })),
            typed("agent.role.v2", json!({ "role": null })),
        ],
        Format::Text,
        &[],
    )
    .expect("text");

    assert_eq!(printed, "Role: Reviewer, Release captain");
    assert_eq!(
        printed
            .lines()
            .filter(|line| line.starts_with("Role"))
            .count(),
        1,
        "more than one role heading: {printed}"
    );
}

/// A step is what to do, what says it is done, and what to do when it is not.
///
/// Everything else a step record carries is bookkeeping about the step rather
/// than the step: who owns it, why it exists, whether it can be undone, which
/// project and process it belongs to, and its own number — which is used to
/// order and to number it, and is not printed twice.
#[test]
fn a_step_prints_its_three_parts_and_no_bookkeeping() {
    let printed = records(
        &[typed(
            "agent.step.v2",
            json!({
                "step": 2,
                "process": "premerge",
                "does": "Read the diff",
                "gate": "Every file opened",
                "on_fail": "Stop and ask",
                "actor": "reviewer",
                "why": "Defects are cheapest here",
                "irreversible": false,
                "project": "equill"
            }),
        )],
        Format::Text,
        &[],
    )
    .expect("text");

    assert_eq!(
        printed,
        "Steps:\n2. Do: Read the diff\n   Gate: Every file opened\n   On fail: Stop and ask"
    );
}

/// A step that broke its own contract is still a step.
///
/// The schema requires an instruction, so one without it is a record that
/// failed to say the one thing it exists to say. An empty `Do:` says both that
/// the step is there and that it says nothing; dropping it would quietly
/// shorten the process, which is the more dangerous of the two.
#[test]
fn a_step_with_no_instruction_is_printed_rather_than_dropped() {
    let printed = records(
        &[typed(
            "agent.step.v2",
            json!({ "step": 5, "gate": "no instruction here" }),
        )],
        Format::Text,
        &[],
    )
    .expect("text");

    assert_eq!(printed, "Steps:\n5. Do:\n   Gate: no instruction here");
}
