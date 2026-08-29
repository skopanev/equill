use super::{misses, record_query};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

fn root() -> PathBuf {
    let path = std::env::temp_dir().join(format!("equill-telemetry-{}", Uuid::now_v7()));
    fs::create_dir_all(&path).expect("directory");
    path
}

/// The log is opt-in, so the test turns it on the way an operator would.
#[test]
fn empty_results_are_the_rows_worth_counting() {
    let root = root();
    // Off until the operator turns it on: nothing is written by default.
    record_query(&root, "search", "unlogged", Vec::new(), 0, false);
    assert!(!root.join("diagnostics/queries.jsonl").exists());

    record_query(&root, "search", "worktree", Vec::new(), 2, true);
    record_query(&root, "search", "worktrees", Vec::new(), 0, true);
    record_query(&root, "context", "sweep", vec!["scope"], 0, true);

    let (total, missed) = misses(&root).expect("read log");
    assert_eq!(total, 3);
    assert_eq!(missed, 2);
    let log = fs::read_to_string(root.join("diagnostics/queries.jsonl")).expect("log");
    assert_eq!(log.lines().count(), 3);
    // Coordinates are recorded by name; their values are the caller's business.
    assert!(log.contains("\"coordinates\":[\"scope\"]"));
    fs::remove_dir_all(root).expect("cleanup");
}

/// A store that was never queried answers zero rather than failing, and a
/// diagnostics write must never be able to break a query that already worked.
#[test]
fn an_absent_log_is_not_an_error() {
    let root = root();
    assert_eq!(misses(&root).expect("absent log"), (0, 0));
    record_query(
        &root.join("missing-store"),
        "search",
        "x",
        Vec::new(),
        1,
        true,
    );
    fs::remove_dir_all(root).expect("cleanup");
}
