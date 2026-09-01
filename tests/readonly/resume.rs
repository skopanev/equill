//! Whether a refused command catches a lagging index up on its way out.
use crate::lagging;
use crate::{READER, existing_record, run, state, store};
use std::time::Duration;

/// A refused command line call does not start the catch-up it would trigger.
///
/// WHAT THIS SHOWS, exactly: a held actor's refused command does not SPAWN a
/// vector catch-up. It is not a claim about data safety — in all runs, refused
/// or not, the ledger and the projection are unchanged, because nothing ever
/// reaches the endpoint. What the guard buys is that a read-only actor does not
/// spend the store's workers or move its bookkeeping.
///
/// WHICH GUARD THIS HOLDS: the one in `lib.rs`, on the command line. The name
/// used to say only "a refused command", which promised the whole property and
/// delivered half of it: remove the guard in `mcp/mod.rs` and this stays green,
/// so a reader who deleted it would conclude the property was untouched. The
/// session's own guard is held by
/// `authority::a_refused_session_call_changes_nothing_and_starts_no_catch_up`,
/// and removing either guard reddens that one — the server is started through
/// the command line, so it sits behind both.
#[test]
fn a_refused_command_line_call_does_not_start_a_catch_up() {
    let root = store();
    let target = existing_record(&root);
    lagging::prepare(&root);
    let (before_ledger, before_index) = state(&root);

    let out = run(
        &root,
        READER,
        &["revoke", "--id", &target, "--comment", "no"],
    );
    assert!(!out.status.success(), "revoke succeeded for a held actor");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("PM_WRITE_DENIED"),
        "revoke refused for another reason: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // A negative needs a window. The worker is forked detached, so "nothing
    // started" measured the instant the command returns is a statement about
    // how fast this machine forks, not about the guard.
    assert!(
        !lagging::starts(&root),
        "the refused command started a catch-up on its way to being refused"
    );
    assert!(
        crate::harness::settles(&root, Duration::from_secs(10)),
        "a worker is still running"
    );
    let (after_ledger, after_index) = state(&root);
    assert_eq!(before_ledger, after_ledger, "the ledger changed");
    assert_eq!(
        before_index, after_index,
        "the refused command caught the index up on its way to being refused"
    );

    // The control: the same store, in the same state, an actor that may. It
    // calibrates the window above — if this cannot start a catch-up within it
    // either, the negative measured nothing.
    lagging::unmute(&root);
    let out = run(
        &root,
        "lane",
        &["revoke", "--id", &target, "--comment", "yes"],
    );
    assert!(
        out.status.success(),
        "the control revoke failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        lagging::starts(&root),
        "nothing resumes for anybody here, so the refusal proved nothing"
    );
    let _ = std::fs::remove_dir_all(&root);
}
