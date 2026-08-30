//! What a write that cannot finish must leave behind.
//!
//! Every one of these injects a real failure at a real boundary — a directory
//! that will not take a file, an index that will not take a row — rather than
//! simulating one, because the question is what the code does when the
//! filesystem says no, and a mock cannot answer it.
mod recovery;

use super::tests::{lesson, store};
use super::{append, append_only};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// A staged receipt that cannot be written means the write did not happen.
///
/// Not "the receipt is missing" — the whole write. A caller told a record is
/// durable is being told the ledger holds it AND that there is a receipt saying
/// so; if the second cannot be produced, the first must not be claimed.
#[test]
fn a_write_whose_receipt_cannot_be_staged_is_refused() {
    let root = store();
    append(&root, lesson("the record before the injection"), "writer").expect("seed");
    let before = receipts(&root);
    let ledger_before = ledger_bytes(&root);

    // The staging directory, not the month: a receipt is staged in
    // `receipts/pending` and renamed into its month on commit, so this is where
    // staging can be made to fail without touching anything already committed.
    let pending = root.join("receipts/pending");
    seal(&pending);
    let outcome = append_only(&root, lesson("the record under test"), "writer");
    unseal(&pending);

    let error = outcome.expect_err("a write whose receipt could not be staged reported success");
    eprintln!("receipt injection refused with: {error}");
    assert_eq!(
        receipts(&root),
        before,
        "a refused write left a receipt behind"
    );
    assert_eq!(
        ledger_bytes(&root),
        ledger_before,
        "a refused write reached the ledger"
    );
    let _ = fs::remove_dir_all(&root);
}

/// The mirror: a ledger that cannot be appended to must leave no receipt.
///
/// The receipt is staged before the append and committed after it, so this is
/// the window where a receipt could outlive the record it describes — a
/// document attesting to something that never became true.
#[test]
fn a_write_that_cannot_reach_the_ledger_leaves_no_receipt() {
    let root = store();
    append(&root, lesson("the record before the injection"), "writer").expect("seed");
    let before = receipts(&root);
    let ledger = ledger_file(&root);

    seal(&ledger);
    let outcome = append_only(&root, lesson("the record under test"), "writer");
    unseal(&ledger);

    let error = outcome.expect_err("a write that could not reach the ledger reported success");
    eprintln!("ledger injection refused with: {error}");
    assert_eq!(
        receipts(&root),
        before,
        "a write that never reached the ledger left a receipt claiming it did"
    );
    let _ = fs::remove_dir_all(&root);
}

/// Make a path unwritable. Read-only rather than absent: a missing path would
/// test error handling for a store that was never built, and what is being
/// injected here is a store that stops accepting writes midway.
pub(super) fn seal(path: &Path) {
    let mut permissions = fs::metadata(path).expect("metadata").permissions();
    permissions.set_mode(0o555);
    fs::set_permissions(path, permissions).expect("seal");
}

pub(super) fn unseal(path: &Path) {
    let mut permissions = fs::metadata(path).expect("metadata").permissions();
    permissions.set_mode(0o755);
    let _ = fs::set_permissions(path, permissions);
}

pub(super) fn month_directory(root: &Path) -> std::path::PathBuf {
    fs::read_dir(root.join("receipts/writes"))
        .expect("receipts")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.is_dir())
        .expect("a month of receipts")
}

pub(super) fn ledger_file(root: &Path) -> std::path::PathBuf {
    fs::read_dir(root.join("records"))
        .expect("records")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|value| value == "jsonl"))
        .expect("a ledger file")
}

pub(super) fn ledger_bytes(root: &Path) -> u64 {
    fs::metadata(ledger_file(root)).expect("ledger").len()
}

/// Committed receipts only. A staged one is a dotfile and is removed when the
/// staging is dropped, which is itself part of what these tests assert.
pub(super) fn receipts(root: &Path) -> usize {
    fs::read_dir(month_directory(root))
        .expect("month")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".json"))
        .count()
}
