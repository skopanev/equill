//! Stages recovery refuses to act on.
//!
//! Each is a thing that could be mistaken for an answer: a stage that disagrees
//! with the ledger, a directory that cannot be listed, a shard that cannot be
//! parsed, a link standing where a staged file should be. None of them says the
//! record is absent, and none may be treated as though it did.
use super::super::super::tests::{lesson, store};
use super::super::super::{append, append_only};
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

pub(super) fn staged_orphan(root: &std::path::Path) -> Uuid {
    let orphan = Uuid::now_v7();
    stage_by_hand(root, orphan, orphan, &"0".repeat(64), recorded_at(root));
    orphan
}

/// The next write is refused, and the stage is neither finished nor abandoned.
pub(super) fn blocked(root: &std::path::Path, orphan: Uuid, injected: &str) {
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

/// A temporary name already taken belongs to whoever took it.
///
/// The note is written through a file claimed with `create_new`, so a name
/// that already exists fails the claim. What must not follow is the cleanup:
/// removing the file on that failure would destroy something this call did not
/// make, on the strength of having collided with it. The name is fresh every
/// time in practice, which is exactly why the collision has to be arranged
/// here — otherwise the branch has no way of being asked about.
#[test]
fn a_temporary_name_already_taken_is_not_destroyed() {
    const PLANTED: &[u8] = b"not this store's file";
    let root = store();
    append(&root, lesson("the record before the injection"), "writer").expect("seed");
    let orphan = staged_orphan(&root);
    let pinned = Uuid::now_v7();
    let abandoned = root.join("receipts/abandoned");
    fs::create_dir_all(&abandoned).expect("abandoned");
    let planted = abandoned.join(format!(".{pinned}.json"));
    fs::write(&planted, PLANTED).expect("plant");
    super::super::super::receipt::quarantine_seam::pin_temp(pinned);

    let refused = append_only(&root, lesson("the write that must wait"), "writer")
        .expect_err("a quarantine whose temporary name was taken");
    assert!(
        matches!(refused, crate::kernel::error::Error::Io(_)),
        "the collision was reported as something else: {refused:?}"
    );
    assert_eq!(
        fs::read(&planted).expect("the planted file is gone"),
        PLANTED,
        "a file this call did not create was destroyed by it"
    );
    assert!(
        root.join(format!("receipts/pending/{orphan}.json"))
            .is_file(),
        "the stage was disposed of after a failed quarantine"
    );
    assert!(
        !root
            .join(format!("receipts/abandoned/{orphan}.json"))
            .exists(),
        "a note was recorded despite the failure"
    );
    let _ = fs::remove_dir_all(&root);
}
