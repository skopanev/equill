//! A staged receipt that survived a crash proves nothing on its own.
//!
//! Staging happens before the ledger append, so a pending file can belong to a
//! write that reached the ledger or to one that never got there. Recovery has
//! to tell those apart by asking the ledger, because finishing the second kind
//! would produce a committed receipt attesting to a record that does not exist
//! — a document more convincing than the truth it contradicts.
mod blocked;
mod confinement;
mod refusals;
mod shard;

use super::super::tests::{lesson, store};
use super::super::{append, append_only};
use super::{ledger_file, receipts};
use serde_json::json;
use std::fs;
use uuid::Uuid;

/// A pending receipt with no record behind it is abandoned, never finished.
#[test]
fn an_orphaned_stage_does_not_become_a_receipt() {
    let root = store();
    append(&root, lesson("the only real record"), "writer").expect("seed");
    let committed = receipts(&root);
    let ledger_before = lines(&root);

    // What a crash between staging and the append leaves: a well-formed receipt
    // for a record the ledger never received.
    let orphan = Uuid::now_v7();
    stage_by_hand(&root, orphan, orphan, &"0".repeat(64), recorded_at(&root));

    append_only(&root, lesson("the write after the crash"), "writer").expect("the next write");

    assert!(
        !root
            .join(format!("receipts/pending/{orphan}.json"))
            .exists(),
        "the orphaned stage is still pending"
    );
    assert!(
        root.join(format!("receipts/abandoned/{orphan}.json"))
            .is_file(),
        "the orphaned stage was destroyed rather than set aside"
    );
    assert!(
        !month_receipt(&root, orphan).exists(),
        "a receipt was committed for a record that was never written"
    );
    assert_eq!(
        receipts(&root),
        committed + 1,
        "recovery committed something of its own"
    );
    assert_eq!(
        lines(&root),
        ledger_before + 1,
        "the ledger gained a record it should not have"
    );
    let _ = fs::remove_dir_all(&root);
}

/// What is kept about an abandoned stage, and what is deliberately not.
///
/// The stage reads like a receipt — namespace, type, actor, findings — for a
/// record that does not exist. Keeping the file would leave that document in a
/// directory a person goes looking through. The note keeps the transaction, the
/// digest of the bytes as they stood, why, and when; nothing that describes a
/// write which never happened.
#[test]
fn an_abandoned_stage_leaves_a_note_and_nothing_else() {
    let root = store();
    append(&root, lesson("the only real record"), "writer").expect("seed");
    let orphan = Uuid::now_v7();
    let staged = stage_by_hand(&root, orphan, orphan, &"0".repeat(64), recorded_at(&root));

    append_only(&root, lesson("the write after the crash"), "writer").expect("the next write");

    let note = note_for(&root, orphan);
    let keys: Vec<&str> = note
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "coordinate",
            "reason",
            "recovered_at",
            "schema",
            "stage_sha256"
        ],
        "the note carries fields beyond what an abandoned stage is allowed to keep"
    );
    assert_eq!(note["reason"], "pre_append_crash");
    assert_eq!(note["schema"], "equill.receipt-quarantine.v1");
    // The coordinate the receipt would have occupied, in the form a successful
    // write reports back — not a bare id.
    let month = ledger_file(&root)
        .file_stem()
        .expect("month")
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        note["coordinate"],
        format!("receipts/writes/{month}/{orphan}.json")
    );
    assert_eq!(
        note["stage_sha256"],
        crate::kernel::digest::sha256_hex(&staged),
        "the note does not describe the bytes it replaced"
    );
    assert!(
        !root
            .join(format!("receipts/pending/{orphan}.json"))
            .exists(),
        "the stage survived the note that replaced it"
    );

    // A crash between writing the note and removing the stage leaves both. The
    // next run must reach the same place rather than depend on having finished.
    fs::write(
        root.join(format!("receipts/pending/{orphan}.json")),
        &staged,
    )
    .expect("restage");
    append_only(&root, lesson("the write after the second crash"), "writer").expect("write");
    // Everything but the time it happened: a second recovery really did happen
    // at a second time, and a note claiming otherwise would be the lie. What
    // must not move is what the note says about the transaction.
    let again = note_for(&root, orphan);
    for field in ["schema", "coordinate", "stage_sha256", "reason"] {
        assert_eq!(
            again[field], note[field],
            "repeating the recovery changed {field}"
        );
    }
    assert!(
        again["recovered_at"]
            .as_str()
            .is_some_and(|value| value.parse::<jiff::Timestamp>().is_ok()),
        "the repeated recovery did not record when it happened"
    );
    assert!(
        !root
            .join(format!("receipts/pending/{orphan}.json"))
            .exists(),
        "the repeated recovery left the stage behind"
    );
    let _ = fs::remove_dir_all(&root);
}

fn note_for(root: &std::path::Path, id: Uuid) -> serde_json::Value {
    serde_json::from_slice(
        &fs::read(root.join(format!("receipts/abandoned/{id}.json"))).expect("a note"),
    )
    .expect("json")
}

pub(super) fn stage_by_hand(
    root: &std::path::Path,
    receipt_id: Uuid,
    record_id: Uuid,
    digest: &str,
    recorded_at: String,
) -> Vec<u8> {
    let directory = root.join("receipts/pending");
    fs::create_dir_all(&directory).expect("pending");
    let bytes = serde_json::to_vec(&json!({
        "receipt_id": receipt_id,
        "status": "appended",
        "record_id": record_id,
        "namespace": "agent.memory",
        "type": "agent.lesson.v1",
        "actor": "writer",
        "recorded_at": recorded_at,
        "record_sha256": digest,
        "durable": true,
        "projection": "not-applicable",
        "defense_findings": [],
    }))
    .expect("json");
    fs::write(directory.join(format!("{receipt_id}.json")), &bytes).expect("stage");
    bytes
}

fn month_receipt(root: &std::path::Path, id: Uuid) -> std::path::PathBuf {
    let month = ledger_file(root)
        .file_stem()
        .expect("month")
        .to_string_lossy()
        .into_owned();
    root.join(format!("receipts/writes/{month}/{id}.json"))
}

pub(super) fn last(root: &std::path::Path) -> serde_json::Value {
    let contents = fs::read_to_string(ledger_file(root)).expect("ledger");
    serde_json::from_str(contents.lines().next_back().expect("a record")).expect("json")
}

pub(super) fn last_id(root: &std::path::Path) -> Uuid {
    last(root)["id"]
        .as_str()
        .expect("id")
        .parse()
        .expect("uuid")
}

pub(super) fn recorded_at(root: &std::path::Path) -> String {
    last(root)["recorded_at"]
        .as_str()
        .expect("recorded_at")
        .to_owned()
}

fn lines(root: &std::path::Path) -> usize {
    fs::read_to_string(ledger_file(root))
        .expect("ledger")
        .lines()
        .count()
}
