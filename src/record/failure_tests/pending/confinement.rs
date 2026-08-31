//! Every directory the write path touches has to be one the store owns.
//!
//! Checking that a staged file is a regular file says nothing about how it was
//! reached. Any directory on the way — `receipts`, `receipts/pending`, the
//! month a receipt is renamed into, `records` itself — can be a link, and then
//! every file inside it is an ordinary file somewhere else. Recovery renames
//! and deletes, so a directory pointing outside the store makes those renames
//! and deletions happen outside the store.
//!
//! Each test here replaces one component with a link to somewhere outside,
//! puts a sentinel there, and asks for two things: the write is refused, and
//! the sentinel is exactly as it was.
use super::super::super::tests::{lesson, store};
use super::super::super::{append, append_only};
use super::super::ledger_file;
use super::{recorded_at, stage_by_hand};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const SENTINEL: &[u8] = b"outside the store, and none of its business";

/// The staging directory itself.
#[test]
fn a_linked_staging_directory_is_refused_before_anything_is_written() {
    let root = store();
    append(&root, lesson("the record before the injection"), "writer").expect("seed");
    let outside = elsewhere("staging");
    fs::remove_dir_all(root.join("receipts/pending")).expect("clear");
    link(&outside.join("target"), &root.join("receipts/pending"));

    let refused = append_only(&root, lesson("the write that must be refused"), "writer")
        .expect_err("a write through a linked staging directory");
    assert!(
        matches!(refused, crate::kernel::error::Error::Integrity(_)),
        "a linked staging directory was written through: {refused:?}"
    );
    intact(&outside);
    clean(&root, &outside);
}

/// The directory an abandoned stage's note would be written into.
#[test]
fn a_linked_quarantine_directory_is_refused_and_the_stage_is_kept() {
    let root = store();
    append(&root, lesson("the record before the injection"), "writer").expect("seed");
    let orphan = Uuid::now_v7();
    stage_by_hand(&root, orphan, orphan, &"0".repeat(64), recorded_at(&root));
    let outside = elsewhere("quarantine");
    link(&outside.join("target"), &root.join("receipts/abandoned"));

    refused_and_stage_kept(&root, orphan, "a linked quarantine directory");
    intact(&outside);
    clean(&root, &outside);
}

/// The month directory a recovered receipt is renamed into.
#[test]
fn a_linked_month_directory_is_refused_and_the_stage_is_kept() {
    let root = store();
    append(
        &root,
        lesson("the record the stage really describes"),
        "writer",
    )
    .expect("seed");
    let (id, digest) = last_record(&root);
    stage_by_hand(&root, id, id, &digest, recorded_at(&root));
    let month = month_of(&root);
    let outside = elsewhere("month");
    fs::remove_dir_all(root.join("receipts/writes").join(&month)).expect("clear");
    link(
        &outside.join("target"),
        &root.join("receipts/writes").join(&month),
    );

    refused_and_stage_kept(&root, id, "a linked month directory");
    intact(&outside);
    clean(&root, &outside);
}

/// The ledger the shard is read from.
#[test]
fn a_linked_records_directory_is_refused_and_the_stage_is_kept() {
    let root = store();
    append(&root, lesson("the record before the injection"), "writer").expect("seed");
    let orphan = Uuid::now_v7();
    stage_by_hand(&root, orphan, orphan, &"0".repeat(64), recorded_at(&root));
    let outside = elsewhere("records");
    fs::rename(root.join("records"), outside.join("records")).expect("move aside");
    link(&outside.join("records"), &root.join("records"));

    refused_and_stage_kept(&root, orphan, "a linked records directory");
    intact(&outside);
    clean(&root, &outside);
}

/// A hard link is not a link, as far as the file type is concerned.
///
/// It is an ordinary regular file with a second name, and the second name is
/// outside the store. Recovery reads what it finds there and acts on it, so a
/// file the store never wrote would be finished into its receipts. The type
/// check cannot see this; the link count can.
#[test]
fn a_hard_link_in_the_staging_directory_is_refused() {
    let root = store();
    append(&root, lesson("the record before the injection"), "writer").expect("seed");
    let outside = elsewhere("hardlink");
    let id = Uuid::now_v7();
    let planted = outside.join("target").join("planted.json");
    fs::write(&planted, br#"{"receipt_id":"x","status":"appended"}"#).expect("plant");
    let pending = root.join("receipts/pending");
    fs::create_dir_all(&pending).expect("pending");
    fs::hard_link(&planted, pending.join(format!("{id}.json"))).expect("hard link");

    let refused = append_only(&root, lesson("the write that must be refused"), "writer")
        .expect_err("a write over a hard-linked stage");
    assert!(
        matches!(refused, crate::kernel::error::Error::Integrity(_)),
        "a hard-linked stage was acted on: {refused:?}"
    );
    assert!(
        planted.is_file(),
        "the file outside the store was disturbed"
    );
    intact_sentinel(&outside);
    clean(&root, &outside);
}

fn refused_and_stage_kept(root: &Path, id: Uuid, injected: &str) {
    let refused = append_only(root, lesson("the write that must be refused"), "writer")
        .expect_err("a write through a linked directory");
    assert!(
        matches!(refused, crate::kernel::error::Error::Integrity(_)),
        "{injected} was followed: {refused:?}"
    );
    assert!(
        root.join(format!("receipts/pending/{id}.json")).is_file(),
        "{injected} cost the stage that was waiting to be resolved"
    );
    assert!(
        !root.join(format!("receipts/abandoned/{id}.json")).exists(),
        "{injected} produced an abandonment anyway"
    );
}

/// Somewhere outside the store: one file nothing may touch, and one empty
/// directory for the link to point at.
///
/// The link points at `target` rather than at this directory, so that a store
/// which followed the link would find nothing in its way and go through with
/// the write. Pointing it here instead would leave the sentinel sitting where
/// recovery looks for staged receipts, and every one of these tests would pass
/// on "that is not a receipt" while proving nothing about links.
fn elsewhere(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "equill-outside-{name}-{}-{}",
        std::process::id(),
        Uuid::now_v7()
    ));
    fs::create_dir_all(path.join("target")).expect("outside");
    fs::write(path.join("sentinel"), SENTINEL).expect("sentinel");
    path
}

fn link(target: &Path, at: &Path) {
    if let Some(parent) = at.parent() {
        fs::create_dir_all(parent).expect("parent");
    }
    std::os::unix::fs::symlink(target, at).expect("link");
}

fn intact_sentinel(outside: &Path) {
    assert_eq!(
        fs::read(outside.join("sentinel")).expect("the sentinel is gone"),
        SENTINEL,
        "something reached outside the store"
    );
}

fn intact(outside: &Path) {
    assert_eq!(
        fs::read(outside.join("sentinel")).expect("the sentinel is gone"),
        SENTINEL,
        "something reached outside the store"
    );
    assert!(
        strays(&outside.join("target")).is_empty(),
        "the store wrote outside itself: {:?}",
        strays(&outside.join("target"))
    );
}

fn strays(directory: &Path) -> Vec<String> {
    fs::read_dir(directory)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

fn month_of(root: &Path) -> String {
    ledger_file(root)
        .file_stem()
        .expect("month")
        .to_string_lossy()
        .into_owned()
}

/// The last record's id and the digest the writer would have stated for it.
fn last_record(root: &Path) -> (Uuid, String) {
    let contents = fs::read_to_string(ledger_file(root)).expect("ledger");
    let line = contents.lines().next_back().expect("a record");
    let value: serde_json::Value = serde_json::from_str(line).expect("json");
    (
        value["id"].as_str().expect("id").parse().expect("uuid"),
        crate::kernel::digest::sha256_hex(line.as_bytes()),
    )
}

fn clean(root: &Path, outside: &Path) {
    let _ = fs::remove_file(root.join("records"));
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(outside);
}
