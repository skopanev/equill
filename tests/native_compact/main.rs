//! Compacting a store that has no manifest under it.
//!
//! The manifest path rebuilds a store from the inputs that made it. A store
//! written through `record` has none, so the ledger is both source and target —
//! which is why this is an end-to-end suite: the interesting failures are about
//! what survives a rewrite of the file being read.
#[path = "../harness/mod.rs"]
mod harness;
#[path = "support.rs"]
mod support;

use std::fs;
use support::{ledger_lines, record, run, stderr, stdout, store};

/// A dry run says what would go and changes nothing.
#[test]
fn a_dry_run_reports_the_work_and_leaves_the_store_alone() {
    let root = store("dry-run");
    let first = record(&root, "first version", None);
    let second = record(&root, "second version", Some(&first));
    record(&root, "third version", Some(&second));
    let before = ledger_lines(&root);

    let out = run(&root, &["compact", "--dry-run"]);

    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("Would remove 2 of 3"),
        "{}",
        stdout(&out)
    );
    assert_eq!(
        before,
        ledger_lines(&root),
        "the dry run rewrote the ledger"
    );
    let _ = fs::remove_dir_all(&root);
}

/// After applying, the chain is its living end and nothing points into the
/// past that is gone.
#[test]
fn applying_keeps_the_living_end_and_leaves_no_dangling_link() {
    let root = store("apply");
    let first = record(&root, "first version", None);
    let second = record(&root, "second version", Some(&first));
    let third = record(&root, "third version", Some(&second));
    record(&root, "unrelated", None);

    let out = run(&root, &["compact", "--apply"]);
    assert!(out.status.success(), "{}", stderr(&out));

    let after = ledger_lines(&root);
    assert_eq!(after.len(), 2, "expected two survivors: {after:?}");
    assert!(
        after
            .iter()
            .all(|record| record.get("supersedes").is_none()),
        "a link into the removed past survived: {after:?}"
    );
    assert!(
        after
            .iter()
            .any(|record| record["id"].as_str() == Some(third.as_str())),
        "the living end of the chain was removed"
    );
    let _ = fs::remove_dir_all(&root);
}

/// Removed records are gone, not hidden: neither the reader nor the index
/// serves them, and asking by id is an ordinary not-found.
#[test]
fn a_removed_record_is_not_retrievable_by_any_surface() {
    let root = store("gone");
    let first = record(&root, "sample-removed-text", None);
    record(&root, "sample-surviving-text", Some(&first));

    run(&root, &["compact", "--apply"]);

    let searched = run(
        &root,
        &[
            "search",
            "--query",
            "sample-removed-text",
            "--format",
            "jsonl",
        ],
    );
    assert!(
        stdout(&searched).trim().is_empty(),
        "the index still serves a removed record: {}",
        stdout(&searched)
    );
    let fetched = run(&root, &["get", "--id", &first]);
    assert!(
        !fetched.status.success(),
        "a removed record was still readable"
    );
    assert!(
        !stderr(&fetched).contains("sample-removed-text"),
        "the refusal carried the removed payload: {}",
        stderr(&fetched)
    );
    let _ = fs::remove_dir_all(&root);
}

/// Running it twice removes nothing the second time, and the store the second
/// run sees is the one the first run left.
#[test]
fn compacting_twice_removes_nothing_the_second_time() {
    let root = store("twice");
    let first = record(&root, "first version", None);
    record(&root, "second version", Some(&first));

    run(&root, &["compact", "--apply"]);
    let after_first = ledger_lines(&root);
    let second = run(&root, &["compact", "--dry-run"]);

    assert!(
        stdout(&second).contains("Would remove 0"),
        "{}",
        stdout(&second)
    );
    assert_eq!(after_first, ledger_lines(&root));
    let _ = fs::remove_dir_all(&root);
}

/// The store keeps working afterwards: an ordinary append lands, and a
/// revocation of a survivor still behaves like a revocation.
#[test]
fn the_store_still_accepts_writes_and_revocations_after_compaction() {
    let root = store("after");
    let first = record(&root, "first version", None);
    let survivor = record(&root, "second version", Some(&first));

    run(&root, &["compact", "--apply"]);

    let appended = record(&root, "written after compaction", None);
    assert!(!appended.is_empty());
    let revoked = run(&root, &["revoke", "--id", &survivor]);
    assert!(
        revoked.status.success(),
        "a survivor could not be revoked after compaction: {}",
        stderr(&revoked)
    );
    let _ = fs::remove_dir_all(&root);
}
