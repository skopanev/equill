//! What the selection returned has to reach the answer.
//!
//! The bug this suite exists for was invisible from inside one surface: the
//! selection was right, the receipt was right, and `--format llm` quietly
//! printed less than it was given. Only a comparison between two surfaces of
//! the same call can see that, which is why this is an end-to-end suite and not
//! a unit test.
#[path = "../harness/mod.rs"]
mod harness;
#[path = "support.rs"]
mod support;

use std::fs;
use support::{MARKERS, run, store};

/// Every record the selection returned is named in the prompt.
#[test]
fn no_selected_record_is_missing_from_the_llm_answer() {
    let root = store();
    let structured = run(
        &root,
        &["context", "--profile", "sample", "--format", "jsonl"],
    );
    let prompt = run(
        &root,
        &["context", "--profile", "sample", "--format", "llm"],
    );

    let selected = structured.lines().filter(|line| !line.is_empty()).count();
    assert!(
        selected >= MARKERS.len(),
        "the fixture stopped selecting: {selected}"
    );

    let missing: Vec<&str> = MARKERS
        .into_iter()
        .filter(|marker| structured.contains(marker) && !prompt.contains(marker))
        .collect();
    assert!(
        missing.is_empty(),
        "selection returned these and the prompt dropped them: {missing:?}\n{prompt}"
    );
    let _ = fs::remove_dir_all(&root);
}

/// Two records that read alike and mean different things stay two, and the
/// difference stays visible — collapsing them answers one question where two
/// were asked.
#[test]
fn records_that_read_alike_but_differ_stay_distinguishable() {
    let root = store();
    let prompt = run(
        &root,
        &["context", "--profile", "sample", "--format", "llm"],
    );

    // Association, not presence. Searching the whole prompt for the scope lets
    // any unrelated row carrying that word answer for the lessons — which is
    // how the first version of this test passed while the two lessons were
    // still printed as two identical bullets.
    let block = prompt
        .split("\n\n")
        .find(|block| block.contains("Measure before claiming."))
        .unwrap_or_else(|| panic!("the lesson is missing entirely:\n{prompt}"));

    assert_eq!(
        block.matches("Measure before claiming.").count(),
        2,
        "two differently scoped lessons collapsed:\n{block}"
    );
    for scope in ["scope-only-one", "scope-only-two"] {
        assert!(
            block.contains(scope),
            "{scope} is not shown beside the rule it scopes:\n{block}"
        );
    }
    let _ = fs::remove_dir_all(&root);
}
