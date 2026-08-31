//! The route a refused draft takes.
//!
//! The memory defense turns a draft away before the accepting path's guards
//! run, and a refused draft still writes a receipt — so staging is the first
//! thing to touch the receipts directory there. A write that is refused must
//! also refuse without leaving anything behind outside the store.
use super::super::super::tests::{lesson, store};
use super::super::super::{append, append_only};
use super::confinement::{clean, elsewhere, intact, link};
use std::fs;

/// A refused draft still writes a receipt, so it still touches the path.
///
/// This is the route nobody was looking at. The memory defense refuses the
/// draft and returns before the accepting path's guards run, so staging is the
/// FIRST thing to touch `receipts` — and a directory created there before the
/// walk happens is created wherever `receipts` actually points. The write was
/// refused either way; what must also be true is that nothing appeared outside
/// the store while refusing it.
#[test]
fn a_refused_draft_does_not_create_a_directory_outside_the_store() {
    let root = store();
    append(&root, lesson("the record before the injection"), "writer").expect("seed");
    // First, on an untouched store: this draft really does take the refused
    // path. Without that, the test could be exercising the accepting path and
    // proving nothing about the route it names.
    let blocked = append_only(&root, secret(), "writer").expect_err("a draft the defense refuses");
    assert!(
        matches!(blocked, crate::kernel::error::Error::MemoryDefense(_)),
        "the fixture draft is not refused by the memory defense: {blocked:?}"
    );

    let outside = elsewhere("blocked-draft");
    fs::rename(root.join("receipts"), outside.join("real-receipts")).expect("move aside");
    link(&outside.join("target"), &root.join("receipts"));

    // The refusal itself is not the claim — this draft is refused either way.
    // The claim is that refusing it reached nothing outside the store.
    let _ = append_only(&root, secret(), "writer").expect_err("still refused");
    intact(&outside);
    let _ = fs::remove_file(root.join("receipts"));
    clean(&root, &outside);
}

/// A draft the memory defense will not accept, built from a key shape that
/// exists only to be recognised.
fn secret() -> crate::record::RecordDraft {
    let mut draft = lesson("the rule that carries a key");
    draft.payload = serde_json::json!({ "rule": "AKIA3M7XQZ2WVK5NB4TR is in here" });
    draft
}
