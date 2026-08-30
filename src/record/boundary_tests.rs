//! What confirmation is allowed to do.
//!
//! The contract itself rather than examples of it: confirmation does no
//! rebuildable work, on the internal boundary and on the call a user actually
//! makes. What happens when a write cannot finish lives in `failure_tests`.
use super::append;
use super::tests::{lesson, store};
use std::fs;

/// The confirmation boundary, observed rather than timed.
///
/// A caller is told a record is durable once the ledger holds it and its
/// receipt is committed. Nothing before that point may scan the ledger, rebuild
/// the lifecycle graph, or open a projection transaction: those are rebuildable
/// work, and a write that waits for them is paying for history it already has.
///
/// The end-to-end benchmark measures the consequence — confirmation not getting
/// slower as a store grows. This measures the cause, so a slow machine cannot
/// hide it and a fast one cannot excuse it.
#[test]
fn confirmation_touches_no_rebuildable_work() {
    let root = store();
    // A store with some history, so a scan would have something to find.
    for index in 0..20 {
        append(&root, lesson(&format!("existing lesson {index}")), "writer").expect("seed");
    }

    super::hotpath::reset();
    // append_only IS the confirmation boundary. `append` is the convenience
    // path and does projection work after it, which is where that work belongs
    // and is not what this measures.
    super::append_only(&root, lesson("the record under test"), "writer").expect("append");
    let touched = super::hotpath::touched();

    assert_eq!(
        touched,
        super::hotpath::Touched::default(),
        "confirmation did rebuildable work: {touched:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// The same claim about the call a user actually makes.
///
/// The test above measures `append_only`, which is the boundary itself. This
/// one measures `append` — what the CLI and the MCP server call — because a
/// boundary that is clean only when reached through the internal entry point is
/// not a boundary. The text index used to be written here, after confirmation
/// but before the response, which the seam above could not see.
#[test]
fn the_call_a_user_makes_opens_no_projection_transaction() {
    let root = store();
    for index in 0..20 {
        append(&root, lesson(&format!("existing lesson {index}")), "writer").expect("seed");
    }

    super::hotpath::reset();
    let report = append(&root, lesson("the record under test"), "writer").expect("append");
    let touched = super::hotpath::touched();

    assert_eq!(
        touched,
        super::hotpath::Touched::default(),
        "the user-facing append did rebuildable work: {touched:?}"
    );
    // And it says so. A response reporting the index as ready while the index
    // is not yet written would be the same lie in the other direction.
    assert!(
        matches!(
            report.projection,
            crate::projection::ProjectionState::Queued
        ),
        "the append reported {:?} rather than queued",
        report.projection
    );
    let _ = fs::remove_dir_all(&root);
}
