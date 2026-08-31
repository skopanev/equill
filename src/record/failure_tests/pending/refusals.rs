//! Stages recovery refuses to act on.
//!
//! Each is a thing that could be mistaken for an answer: a stage that disagrees
//! with the ledger, a directory that cannot be listed, a shard that cannot be
//! parsed, a link standing where a staged file should be. None of them says the
//! record is absent, and none may be treated as though it did.
use super::super::super::tests::{lesson, store};
use super::super::super::{append, append_only};
use super::super::ledger_file;
use super::{last_id, recorded_at, stage_by_hand};
use std::fs;
use uuid::Uuid;

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

/// Only files this store staged are resolvable.
///
/// A link in the staging directory is read through by an ordinary open, and
/// what recovery does after reading is RENAME — so a link would finish
/// something from outside the store as though it had been staged by it.
#[test]
fn a_link_in_the_staging_directory_is_refused() {
    let root = store();
    append(&root, lesson("the record before the injection"), "writer").expect("seed");
    let id = last_id(&root);
    let elsewhere = root.join("outside.json");
    fs::write(&elsewhere, b"{}").expect("outside");
    let link = root.join(format!("receipts/pending/{id}.json"));
    fs::create_dir_all(link.parent().expect("pending")).expect("pending");
    std::os::unix::fs::symlink(&elsewhere, &link).expect("link");

    let refused = append_only(&root, lesson("the write that must wait"), "writer")
        .expect_err("a write over a linked stage");
    assert!(
        matches!(refused, crate::kernel::error::Error::Integrity(_)),
        "a link was accepted as a staged receipt: {refused:?}"
    );
    assert!(
        fs::symlink_metadata(&link).is_ok(),
        "the link was disposed of rather than refused"
    );
    assert!(elsewhere.is_file(), "the link's target was moved");
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

/// A digest that cannot be a digest makes the ledger's answer meaningless.
///
/// Checked before the shard is opened. Otherwise a stage carrying nonsense here
/// and naming a record that is genuinely absent would come out as an ordinary
/// pre-append crash and be quarantined as though it had been understood.
#[test]
fn a_digest_that_is_not_a_sha256_blocks() {
    let root = store();
    append(&root, lesson("the record before the injection"), "writer").expect("seed");
    let orphan = Uuid::now_v7();
    // Sixty-four hexadecimal characters, uppercase. Close enough to look right
    // and not what this store writes.
    stage_by_hand(&root, orphan, orphan, &"A".repeat(64), recorded_at(&root));

    blocked(&root, orphan, "an uppercase digest");
    let _ = fs::remove_dir_all(&root);
}

fn staged_orphan(root: &std::path::Path) -> Uuid {
    let orphan = Uuid::now_v7();
    stage_by_hand(root, orphan, orphan, &"0".repeat(64), recorded_at(root));
    orphan
}

/// The next write is refused, and the stage is neither finished nor abandoned.
fn blocked(root: &std::path::Path, orphan: Uuid, injected: &str) {
    let refused = append_only(root, lesson("the write that must wait"), "writer")
        .expect_err("a write over a stage recovery cannot resolve");
    assert!(
        matches!(refused, crate::kernel::error::Error::Integrity(_)),
        "{injected} was not refused: {refused:?}"
    );
    assert!(
        root.join(format!("receipts/pending/{orphan}.json"))
            .is_file(),
        "{injected} led to the stage being disposed of"
    );
    assert!(
        !root
            .join(format!("receipts/abandoned/{orphan}.json"))
            .exists(),
        "{injected} was read as the record being absent"
    );
}
