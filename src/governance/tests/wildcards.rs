use super::super::{grant, show, transfer};
use super::store;
use crate::command::init;
use crate::kernel::error::Error;
use crate::kernel::store as config;
use std::path::PathBuf;

/// A store-wide `*` writer is an authority a handover cannot take away: it means
/// "any valid actor", so the previous owner keeps append access through it no
/// matter what is removed from the lists.
///
/// Deleting the wildcard instead would be worse than the hole — every other
/// actor relying on it would lose access without being named. So the handover
/// refuses, says what is in the way, and changes nothing.
#[test]
fn a_handover_refuses_while_a_wildcard_writer_exists() {
    let root = open("*-writer", &["founder".into(), "*".into()]);
    let before = std::fs::read(root.join("store.json")).expect("metadata");

    let refused = transfer(&root, "successor", None, "founder");

    let message = match refused {
        Err(Error::Governance(message)) => message,
        other => panic!("expected a named refusal, got {other:?}"),
    };
    assert!(message.contains("`*` writer"), "{message}");
    assert_eq!(
        std::fs::read(root.join("store.json")).expect("metadata"),
        before,
        "a refused handover must not touch the metadata"
    );
    assert_eq!(config::load(&root).expect("load").root_owner, "founder");
    assert!(super::transfer::audit_records(&root).is_empty());
}

/// The same hole in a smaller shape: a scoped grant whose actor list is `*`.
#[test]
fn a_handover_refuses_while_a_wildcard_grant_exists() {
    let root = store();
    grant(
        &root,
        "*",
        "agent.memory",
        &["agent.lesson.v1".into()],
        None,
        "founder",
    )
    .expect("a wildcard grant is a legitimate thing to hold");
    let before = std::fs::read(root.join("store.json")).expect("metadata");

    let refused = transfer(&root, "successor", None, "founder");

    let message = match refused {
        Err(Error::Governance(message)) => message,
        other => panic!("expected a named refusal, got {other:?}"),
    };
    assert!(message.contains("`*` grant on agent.memory"), "{message}");
    assert_eq!(
        std::fs::read(root.join("store.json")).expect("metadata"),
        before
    );
    assert_eq!(show(&root).expect("show").grants.len(), 1);
}

/// And once the wildcard is replaced by named actors, the handover goes through
/// — the refusal is a blocked path, not a dead end.
#[test]
fn naming_the_actors_explicitly_unblocks_the_handover() {
    let root = store();
    grant(
        &root,
        "*",
        "agent.memory",
        &["agent.lesson.v1".into()],
        None,
        "founder",
    )
    .expect("grant");
    assert!(transfer(&root, "successor", None, "founder").is_err());

    super::super::revoke_grant(&root, "*", None, "founder").expect("withdraw the wildcard");
    grant(
        &root,
        "worker",
        "agent.memory",
        &["agent.lesson.v1".into()],
        None,
        "founder",
    )
    .expect("name the actor instead");

    transfer(&root, "successor", None, "founder").expect("now it goes through");
    assert_eq!(config::load(&root).expect("load").root_owner, "successor");
}

fn open(name: &str, writers: &[String]) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "equill-governance-{}-{name}-{}",
        std::process::id(),
        uuid::Uuid::now_v7().simple()
    ));
    let _ = std::fs::remove_dir_all(&root);
    init::create_with_writers(&root, "founder", "agent.memory", writers).expect("initialize");
    root
}
