use super::super::{show, transfer};
use super::store;
use crate::command::init;
use crate::kernel::store as config;

/// `init` remains the wrong tool for a handover, and refuses to be used as one.
/// It is what forced this module to exist: before it, changing an owner meant
/// editing metadata by hand.
#[test]
fn init_still_refuses_to_re_own_an_existing_store() {
    let root = store();
    let before = config::load(&root).expect("load");

    let refused = init::create_with_writers(&root, "successor", "agent.memory", &[]);

    assert!(refused.is_err(), "init must not re-own a store");
    let after = config::load(&root).expect("the store still loads");
    assert_eq!(after.root_owner, before.root_owner);
    assert_eq!(after.writers, before.writers);
}

/// Re-running init with the owner it already has is the idempotent case, and
/// must not quietly undo a handover that happened since.
#[test]
fn init_cannot_walk_a_handover_back() {
    let root = store();
    transfer(&root, "successor", None, "founder").expect("transfer");

    let refused = init::create_with_writers(&root, "founder", "agent.memory", &[]);

    assert!(refused.is_err());
    assert_eq!(show(&root).expect("show").owner, "successor");
}
