//! A staged receipt that survived a crash proves nothing on its own.
//!
//! Staging happens before the ledger append, so a pending file can belong to a
//! write that reached the ledger or to one that never got there. Recovery has
//! to tell those apart by asking the ledger, because finishing the second kind
//! would produce a committed receipt attesting to a record that does not exist
//! — a document more convincing than the truth it contradicts.
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

/// A pending receipt that does not match the record it names blocks the store.
#[test]
fn a_stage_that_disagrees_with_the_ledger_blocks_the_next_write() {
    let root = store();
    append(&root, lesson("the record it will misdescribe"), "writer").expect("seed");
    let id = last_id(&root);
    // Same record, wrong digest: a stage that cannot be reconciled with what the
    // ledger actually holds.
    stage_by_hand(&root, id, id, &"a".repeat(64), recorded_at(&root));

    let refused = append_only(&root, lesson("the write that must wait"), "writer")
        .expect_err("a write over an unreconcilable stage");
    assert!(
        matches!(refused, crate::kernel::error::Error::Integrity(_)),
        "the blocked write failed for the wrong reason: {refused:?}"
    );
    // Left where it is: not finished, not quarantined. Recovery can neither
    // confirm nor dismiss it, so it stays for a person to look at.
    assert!(
        root.join(format!("receipts/pending/{id}.json")).is_file(),
        "a stage recovery could not reconcile was disposed of anyway"
    );
    assert!(
        !root.join(format!("receipts/abandoned/{id}.json")).exists(),
        "a stage naming a record that IS in the ledger was treated as orphaned"
    );
    let _ = fs::remove_dir_all(&root);
}

/// A staging directory that cannot be read is not an empty one.
#[test]
fn a_pending_directory_that_cannot_be_read_blocks_the_next_write() {
    let root = store();
    append(&root, lesson("the record before the injection"), "writer").expect("seed");
    let pending = root.join("receipts/pending");

    let mut permissions = fs::metadata(&pending).expect("metadata").permissions();
    let restore = permissions.clone();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o000);
    fs::set_permissions(&pending, permissions).expect("seal");
    let refused = append_only(&root, lesson("the write that must wait"), "writer");
    fs::set_permissions(&pending, restore).expect("restore");

    let refused = refused.expect_err("a write over an unreadable staging directory");
    assert!(
        matches!(refused, crate::kernel::error::Error::Io(_)),
        "an unreadable staging directory was read as empty: {refused:?}"
    );
    let _ = fs::remove_dir_all(&root);
}

fn stage_by_hand(
    root: &std::path::Path,
    receipt_id: Uuid,
    record_id: Uuid,
    digest: &str,
    recorded_at: String,
) {
    let directory = root.join("receipts/pending");
    fs::create_dir_all(&directory).expect("pending");
    fs::write(
        directory.join(format!("{receipt_id}.json")),
        serde_json::to_vec(&json!({
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
        .expect("json"),
    )
    .expect("stage");
}

fn month_receipt(root: &std::path::Path, id: Uuid) -> std::path::PathBuf {
    let month = ledger_file(root)
        .file_stem()
        .expect("month")
        .to_string_lossy()
        .into_owned();
    root.join(format!("receipts/writes/{month}/{id}.json"))
}

fn last(root: &std::path::Path) -> serde_json::Value {
    let contents = fs::read_to_string(ledger_file(root)).expect("ledger");
    serde_json::from_str(contents.lines().next_back().expect("a record")).expect("json")
}

fn last_id(root: &std::path::Path) -> Uuid {
    last(root)["id"]
        .as_str()
        .expect("id")
        .parse()
        .expect("uuid")
}

fn recorded_at(root: &std::path::Path) -> String {
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
