//! The ledger is the one path whose loss cannot be repaired.
//!
//! A receipt that goes astray can be rebuilt from the ledger. A record that
//! goes astray can be rebuilt from nothing — the ledger is what everything else
//! is derived from. So an append that lands outside the store is worse than any
//! of the receipt failures, and worse still because it reports success: the
//! caller is told the record is durable at the moment it went somewhere the
//! store will never look.
use super::super::tests::{lesson, store};
use super::super::{append, append_only};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// A shard replaced by a link to a file outside the store.
#[test]
fn a_linked_ledger_shard_does_not_take_the_append_outside_the_store() {
    let root = store();
    append(
        &root,
        lesson("the record that establishes the shard"),
        "writer",
    )
    .expect("seed");
    let outside = elsewhere();
    let shard = shard(&root);
    let moved = outside.join("shard.jsonl");
    fs::rename(&shard, &moved).expect("move the shard aside");
    std::os::unix::fs::symlink(&moved, &shard).expect("link");
    let before = fs::metadata(&moved).expect("shard").len();

    let refused = append_only(&root, lesson("the record that must not leave"), "writer")
        .expect_err("an append through a linked shard");

    assert!(
        matches!(refused, crate::kernel::error::Error::Integrity(_)),
        "an append through a linked shard failed for the wrong reason: {refused:?}"
    );
    assert_eq!(
        fs::metadata(&moved).expect("shard").len(),
        before,
        "the record was appended to a file outside the store"
    );
    clean(&root, &outside);
}

/// The directory holding the shards, replaced the same way.
#[test]
fn a_linked_records_directory_does_not_take_the_append_outside_the_store() {
    let root = store();
    append(
        &root,
        lesson("the record that establishes the shard"),
        "writer",
    )
    .expect("seed");
    let outside = elsewhere();
    let records = root.join("records");
    fs::rename(&records, outside.join("records")).expect("move the directory aside");
    std::os::unix::fs::symlink(outside.join("records"), &records).expect("link");
    let moved = outside.join("records").join(
        shard(&root)
            .file_name()
            .expect("shard name")
            .to_string_lossy()
            .into_owned(),
    );
    let before = fs::metadata(&moved).expect("shard").len();

    let refused = append_only(&root, lesson("the record that must not leave"), "writer")
        .expect_err("an append through a linked records directory");

    assert!(
        matches!(refused, crate::kernel::error::Error::Integrity(_)),
        "an append through a linked records directory failed for the wrong reason: {refused:?}"
    );
    assert_eq!(
        fs::metadata(&moved).expect("shard").len(),
        before,
        "the record was appended to a file outside the store"
    );
    let _ = fs::remove_file(&records);
    clean(&root, &outside);
}

fn shard(root: &Path) -> PathBuf {
    fs::read_dir(root.join("records"))
        .expect("records")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|value| value == "jsonl"))
        .expect("a shard")
}

fn elsewhere() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "equill-outside-ledger-{}-{}",
        std::process::id(),
        Uuid::now_v7()
    ));
    fs::create_dir_all(&path).expect("outside");
    path
}

fn clean(root: &Path, outside: &Path) {
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(outside);
}
