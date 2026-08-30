use super::super::transfer;
use super::store;
use crate::kernel::error::Error;
use crate::kernel::governance::RootGuard;
use crate::schema::{self, TypeDefinition};
use serde_json::json;
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// A root-governed operation holds the governance guard for its whole run, so a
/// handover cannot land in the middle of one. This drives the ordering: the
/// guard is taken and held while a transfer is attempted from another thread,
/// and the transfer is only allowed to complete once the guard is released.
#[test]
fn an_operation_in_flight_blocks_a_handover_until_it_finishes() {
    let root = store();
    let (guard, _config) = RootGuard::acquire(&root, "founder").expect("the owner governs");

    let path = root.clone();
    let (done, wait) = mpsc::channel();
    let handover = thread::spawn(move || {
        let outcome = transfer(&path, "successor", None, "founder");
        done.send(()).expect("signal");
        outcome
    });

    // While the guard is held the handover cannot proceed. This is a negative
    // observation, so it is bounded rather than instant.
    assert!(
        wait.recv_timeout(Duration::from_millis(300)).is_err(),
        "a handover must not land while an operation holds the guard"
    );
    drop(guard);

    handover
        .join()
        .expect("thread")
        .expect("and goes through once the operation finishes");
    assert_eq!(
        crate::kernel::store::load(&root).expect("load").root_owner,
        "successor"
    );
}

/// The other order: once a handover has landed, an operation started by the old
/// root fails when it asks for authority — before it changes anything.
#[test]
fn a_handover_first_makes_the_old_root_fail_before_it_mutates() {
    let root = store();
    transfer(&root, "successor", None, "founder").expect("handover");
    let before = registry_entries(&root);

    let refused = schema::register(&root, definition(), "founder");

    assert!(
        matches!(refused, Err(Error::PermissionDenied)),
        "{refused:?}"
    );
    assert_eq!(
        registry_entries(&root),
        before,
        "a refused registration must leave the registry untouched"
    );
    schema::register(&root, definition(), "successor").expect("the new root governs");
}

/// Governance operations serialize against each other too: the guard is one
/// lock, so two handovers cannot interleave. The second sees the store the first
/// left behind and is refused on authority.
#[test]
fn two_handovers_cannot_interleave() {
    let root = store();
    transfer(&root, "successor", None, "founder").expect("the first lands");

    let second = transfer(&root, "third", None, "founder");

    assert!(matches!(second, Err(Error::PermissionDenied)), "{second:?}");
    assert_eq!(
        crate::kernel::store::load(&root).expect("load").root_owner,
        "successor"
    );
}

fn definition() -> TypeDefinition {
    TypeDefinition {
        type_name: "agent.lesson.v1".into(),
        uri: "equill://agent.lesson/v1".into(),
        owner: "founder".into(),
        payload_schema: json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "properties": { "rule": { "type": "string" } },
            "required": ["rule"],
            "additionalProperties": false
        }),
        lifecycle: Default::default(),
    }
}

fn registry_entries(root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root.join("registry/types")) else {
        return Vec::new();
    };
    let mut names = entries
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    names.sort();
    names
}
