//! The two levels under `receipts` that a store is not initialized with.
//!
//! A store gets `receipts`; it does not get `receipts/writes`, and it never
//! gets a month until one is written. So the first receipt of all creates two
//! levels, and each of their names has to be made durable before the thing
//! inside it is — outermost first.
use super::super::super::tests::{lesson, store};
use super::super::super::{append, append_only};
use super::super::{ledger_file, month_directory};
use std::fs;

/// A month whose name could not be published has no receipt in it.
#[test]
fn a_fresh_month_that_cannot_be_published_is_not_reported_as_committed() {
    use crate::kernel::path::{Step, fail, reset};

    let root = store();
    reset();
    fail(Step::MonthCreated);
    let refused = append_only(&root, lesson("the first record of its month"), "writer")
        .expect_err("a month whose name could not be made durable");
    assert!(
        matches!(refused, crate::kernel::error::Error::PostCommit(_)),
        "an unpublished month came back as a completed write: {refused:?}"
    );
    reset();
    let _ = fs::remove_dir_all(&root);
}

/// Recovery finishes a receipt into a month that may not exist any more.
///
/// The same first-creation case as an ordinary write, on the path that runs
/// after a crash — which is exactly when a month directory can be missing.
#[test]
fn recovery_publishes_the_name_of_a_month_it_has_to_create() {
    use crate::kernel::path::{Step, reset, steps};

    let root = store();
    append(&root, lesson("the record the stage describes"), "writer").expect("seed");
    let month = month_directory(&root);
    let contents = fs::read_to_string(ledger_file(&root)).expect("ledger");
    let line = contents.lines().next_back().expect("a record");
    let value: serde_json::Value = serde_json::from_str(line).expect("json");
    let id = value["id"].as_str().expect("id");
    // The receipt and its whole month, gone the way a crash could take them.
    fs::remove_dir_all(&month).expect("remove the month");
    fs::create_dir_all(root.join("receipts/pending")).expect("pending");
    fs::write(
        root.join(format!("receipts/pending/{id}.json")),
        serde_json::to_vec(&serde_json::json!({
            "receipt_id": id,
            "status": "appended",
            "record_id": id,
            "recorded_at": value["recorded_at"],
            "record_sha256": crate::kernel::digest::sha256_hex(line.as_bytes()),
            "durable": true,
        }))
        .expect("json"),
    )
    .expect("stage");

    reset();
    append_only(&root, lesson("the write that triggers recovery"), "writer").expect("write");
    assert_eq!(
        steps().first(),
        Some(&Step::MonthCreated),
        "recovery created a month without publishing its name: {:?}",
        steps()
    );
    reset();
    let _ = fs::remove_dir_all(&root);
}

/// A store is initialized with `receipts` and not with `receipts/writes`, so
/// the first receipt of all creates two levels rather than one.
///
/// The outer name has to be published before the inner one: a crash that took
/// `receipts/writes` would take the month and the receipt inside it, while the
/// removal of the stage — published afterwards — would have survived, leaving a
/// durable record with no receipt and nothing to rebuild one from. The second
/// receipt finds both levels there and pays for neither.
#[test]
fn the_first_receipt_of_a_store_publishes_the_name_of_the_writes_directory() {
    use crate::kernel::path::{Step, fail, reset, steps};

    let root = store();
    reset();
    append_only(&root, lesson("the first receipt of the store"), "writer").expect("write");
    let observed = steps();
    let writes = observed
        .iter()
        .position(|step| *step == Step::WritesCreated)
        .expect("the name of receipts/writes was never published");
    let month = observed
        .iter()
        .position(|step| *step == Step::MonthCreated)
        .expect("the name of the month was never published");
    assert!(
        writes < month,
        "the month was published before the directory holding it: {observed:?}"
    );

    reset();
    append_only(&root, lesson("the second receipt of the store"), "writer").expect("write");
    assert!(
        !steps().contains(&Step::WritesCreated),
        "a receipts/writes that already existed was published again: {:?}",
        steps()
    );

    // And an unpublished name is not a committed receipt.
    let fresh = store();
    reset();
    fail(Step::WritesCreated);
    let refused = append_only(
        &fresh,
        lesson("in a store that cannot be published"),
        "writer",
    )
    .expect_err("a receipts directory that could not be published");
    reset();
    assert!(
        matches!(refused, crate::kernel::error::Error::PostCommit(_)),
        "an unpublished receipts/writes came back as a completed write: {refused:?}"
    );
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&fresh);
}
