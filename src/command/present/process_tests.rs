//! Which record gets to name the process, and what a heading leaves out.
use super::tests::typed;
use super::{Format, records};
use serde_json::json;

/// A process is named by its title, and that is the whole of it.
///
/// What the process is for, what it obeys, what it contracts to do — all of it
/// stays in the record and in `--json`. An answer a reader scans for their next
/// action is not where that belongs.
#[test]
fn a_process_prints_its_heading_and_nothing_else() {
    let printed = records(
        &[typed(
            "agent.process.v2",
            json!({
                "title": "Pre-merge audit",
                "name": "premerge",
                "purpose": "Catch defects early",
                "contract": "Every change is read",
                "obeys": "house rules"
            }),
        )],
        Format::Text,
        &[],
    )
    .expect("text");

    assert_eq!(printed, "Process: Pre-merge audit");
}

/// Falling back to the name when there is no title, rather than to whatever
/// sorts first.
#[test]
fn a_process_without_a_title_is_named_by_its_name() {
    let printed = records(
        &[typed(
            "agent.process.v2",
            json!({ "name": "premerge", "purpose": "Catch defects early" }),
        )],
        Format::Text,
        &[],
    )
    .expect("text");

    assert_eq!(printed, "Process: premerge");
}

/// The record that describes the process outranks the one that only refers to
/// it — even when the reference arrives first.
///
/// A role naming `process: "premerge"` is pointing at a process, not defining
/// one. Taking whichever arrived first would head the answer with the code a
/// machine uses instead of the title a person reads, and the order records
/// arrive in is the selector's business, not the reader's.
#[test]
fn the_record_that_describes_the_process_outranks_one_that_only_names_it() {
    let referring_first = records(
        &[
            typed(
                "agent.role.v1",
                json!({ "role": "Reviewer", "process": "premerge" }),
            ),
            typed("agent.process.v2", json!({ "title": "Pre-merge audit" })),
        ],
        Format::Text,
        &[],
    )
    .expect("text");
    let describing_first = records(
        &[
            typed("agent.process.v2", json!({ "title": "Pre-merge audit" })),
            typed(
                "agent.role.v1",
                json!({ "role": "Reviewer", "process": "premerge" }),
            ),
        ],
        Format::Text,
        &[],
    )
    .expect("text");

    assert_eq!(
        referring_first,
        "Role: Reviewer\n\nProcess: Pre-merge audit"
    );
    assert_eq!(
        describing_first, referring_first,
        "the heading depends on which record arrived first"
    );
}

/// A step names the process it belongs to; that reference is not a heading and
/// not a line of the step either.
#[test]
fn a_steps_reference_to_its_process_is_not_a_heading() {
    let printed = records(
        &[typed(
            "agent.step.v2",
            json!({ "step": 1, "process": "premerge", "does": "Read the diff" }),
        )],
        Format::Text,
        &[],
    )
    .expect("text");

    assert_eq!(printed, "Steps:\n1. Do: Read the diff");
}
