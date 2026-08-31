//! A damaged ledger shard cannot say a record is absent.
//!
//! Absence is what sends a stage to quarantine. Reading damage as absence would
//! file the receipt for a record that is really there as an abandoned stage,
//! which is the same fault as manufacturing a receipt, pointed the other way.
use super::super::super::tests::{lesson, store};
use super::super::super::{append, append_only};
use super::super::ledger_file;
use super::refusals::{blocked, staged_orphan};
use super::{recorded_at, stage_by_hand};
use std::fs;
use uuid::Uuid;

/// A ledger shard that cannot be read cannot say the record is absent.
///
/// Reading a parse failure as absence would file a durable record's receipt as
/// an abandoned stage — losing the receipt for a record that is really there,
/// which is the same fault as manufacturing one, pointed the other way.
#[test]
fn a_shard_that_cannot_be_read_blocks_rather_than_answers() {
    let root = store();
    append(&root, lesson("the record before the injection"), "writer").expect("seed");
    let orphan = Uuid::now_v7();
    stage_by_hand(&root, orphan, orphan, &"0".repeat(64), recorded_at(&root));
    // A complete line the reader cannot parse — damage, not a write in progress.
    let ledger = ledger_file(&root);
    let mut contents = fs::read_to_string(&ledger).expect("ledger");
    contents.push_str("{ this is not a record }\n");
    fs::write(&ledger, contents).expect("corrupt");

    let refused = append_only(&root, lesson("the write that must wait"), "writer")
        .expect_err("a write over a shard that cannot answer");
    assert!(
        matches!(refused, crate::kernel::error::Error::Integrity(_)),
        "an unreadable shard was read as absence: {refused:?}"
    );
    assert!(
        root.join(format!("receipts/pending/{orphan}.json"))
            .is_file(),
        "the stage was disposed of on the strength of a shard that could not be read"
    );
    assert!(
        !root
            .join(format!("receipts/abandoned/{orphan}.json"))
            .exists(),
        "an unreadable shard produced an abandonment"
    );
    let _ = fs::remove_dir_all(&root);
}

/// Under the writer lock there is no such thing as a write in progress.
///
/// The ledger reader used elsewhere tolerates an unterminated final line
/// because it runs beside a live writer. Recovery does not: it holds the
/// writer lock, so nothing else can be appending, and an unterminated line is
/// what a crash left. Treating it as benign would let the shard answer
/// "absent" for a record whose own line is the fragment.
#[test]
fn a_shard_with_an_unfinished_final_line_blocks() {
    let root = store();
    append(&root, lesson("the record before the injection"), "writer").expect("seed");
    let orphan = staged_orphan(&root);
    let ledger = super::super::ledger_file(&root);
    // A whole, valid record line — with no newline after it. The fragment has
    // to parse, or this test would be passing on the malformed-line rule and
    // saying nothing about the tail: with the tail check removed, a parseable
    // line whose id is not the one being asked about is skipped, the shard
    // answers "absent", and the stage is quarantined. That is the failure this
    // must detect.
    let contents = fs::read_to_string(&ledger).expect("ledger");
    let whole = contents.lines().next_back().expect("a record").to_owned();
    fs::write(&ledger, format!("{contents}{whole}")).expect("unterminated append");

    blocked(&root, orphan, "an unfinished final line");
    let _ = fs::remove_dir_all(&root);
}

/// A shard holding JSON that is not a record is not a shard this store wrote.
#[test]
fn a_shard_holding_json_that_is_not_a_record_blocks() {
    let root = store();
    append(&root, lesson("the record before the injection"), "writer").expect("seed");
    let orphan = staged_orphan(&root);
    let ledger = super::super::ledger_file(&root);
    let mut contents = fs::read_to_string(&ledger).expect("ledger");
    contents.push_str("{}\n");
    fs::write(&ledger, contents).expect("empty object");

    blocked(&root, orphan, "a line that is not a record");
    let _ = fs::remove_dir_all(&root);
}

/// The writer never emits a blank line, so one is damage rather than nothing.
#[test]
fn a_shard_holding_a_blank_line_blocks() {
    let root = store();
    append(&root, lesson("the record before the injection"), "writer").expect("seed");
    let orphan = staged_orphan(&root);
    let ledger = super::super::ledger_file(&root);
    let mut contents = fs::read_to_string(&ledger).expect("ledger");
    contents.push('\n');
    fs::write(&ledger, contents).expect("blank line");

    blocked(&root, orphan, "a blank line");
    let _ = fs::remove_dir_all(&root);
}
