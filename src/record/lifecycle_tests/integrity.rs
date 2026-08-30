//! The compact state authorizes appends, so it has to be authenticated.
use super::{append, draft, linear, register, store};
use std::fs;

const STATE: &str = "projections/lifecycle/state.jsonl";

/// An edit that keeps the file the same length must not be believed.
///
/// The state's marker used to record only which ledger it was built from — the
/// total byte length. That catches a state left behind by a ledger that moved
/// on, and nothing else: a state edited in place, to the same length, still
/// matched. This is not a theoretical shape. Lifecycle keys are the values the
/// head rules compare, and changing one character of a key is exactly a
/// same-length edit that turns "this key is taken" into "this key is free".
///
/// So the edit here is deliberately the useful one: rename the key the single
/// existing head holds. Under the tampered state a second head claiming the
/// original key looks unopposed and would be authorized. It must not be.
#[test]
fn a_same_length_edit_to_the_state_does_not_authorize_an_append() {
    let root = store("tampered-state");
    register(&root, "agent.lesson.v1", linear(&[]));
    append(&root, draft("agent.lesson.v1", "the head", None), "owner").expect("first head");

    let path = root.join(STATE);
    let before = fs::read_to_string(&path).expect("state");
    // Valid JSON, identical length, and materially different meaning.
    let after = before.replace("\"shared\"", "\"shareX\"");
    assert_ne!(after, before, "the fixture no longer contains the key");
    assert_eq!(
        after.len(),
        before.len(),
        "this test is only about edits a length check cannot see"
    );
    fs::write(&path, &after).expect("tamper");

    // Fail-closed at the load, before anything consults it.
    assert!(
        super::lifecycle::load_state(&root).expect("load").is_none(),
        "a state whose contents do not match its marker was accepted as authority"
    );

    // And the decision that state would have changed comes out the same as it
    // does on an untouched store: the key is taken, so a second head is refused.
    let refused = append(
        &root,
        draft("agent.lesson.v1", "second head", None),
        "owner",
    )
    .expect_err("a second head claiming a taken key");
    assert!(
        refused.to_string().contains("supersedes is required"),
        "refused for the wrong reason: {refused}"
    );
    fs::remove_dir_all(root).expect("cleanup");
}
