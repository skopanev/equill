use super::{LifecyclePolicy, append, draft, linear, read_all, register, store};
use serde_json::json;
use std::fs;
use std::sync::{Arc, Barrier};
use std::thread;

#[test]
fn linear_chain_allows_tombstone_then_correction_and_rejects_stale_head() {
    let root = store("linear");
    register(&root, "agent.lesson.v1", linear(&[]));
    let first = append(&root, draft("agent.lesson.v1", "first", None), "owner")
        .expect("first")
        .id;
    let mut tombstone = draft("agent.lesson.v1", "revoked", Some(first));
    tombstone.tags.push("equill:revoked".into());
    let revoked = append(&root, tombstone, "owner").expect("tombstone").id;
    append(
        &root,
        draft("agent.lesson.v1", "corrected", Some(revoked)),
        "owner",
    )
    .expect("correction after tombstone");

    let stale = append(
        &root,
        draft("agent.lesson.v1", "branch", Some(first)),
        "owner",
    )
    .expect_err("stale target");
    let duplicate = append(&root, draft("agent.lesson.v1", "duplicate", None), "owner")
        .expect_err("duplicate head");

    assert!(stale.to_string().contains("not the current head"));
    assert!(duplicate.to_string().contains("supersedes is required"));
    assert_eq!(read_all(&root).expect("records").len(), 3);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn concurrent_linear_children_have_one_winner() {
    let root = store("race");
    register(&root, "agent.lesson.v1", linear(&[]));
    let head = append(&root, draft("agent.lesson.v1", "head", None), "owner")
        .expect("head")
        .id;
    let barrier = Arc::new(Barrier::new(3));
    let attempts = ["left", "right"].map(|rule| {
        let root = root.clone();
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            barrier.wait();
            append(&root, draft("agent.lesson.v1", rule, Some(head)), "owner")
        })
    });
    barrier.wait();
    let results = attempts.map(|attempt| attempt.join().expect("writer thread"));

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
    assert_eq!(read_all(&root).expect("records").len(), 2);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn linear_head_scan_skips_a_predecessor_without_the_lifecycle_key() {
    let root = store("keyless");
    register(&root, "agent.lesson.v1", LifecyclePolicy::default());
    register(&root, "agent.lesson.v2", linear(&["agent.lesson.v1"]));
    let mut legacy = draft("agent.lesson.v1", "legacy", None);
    legacy.payload = json!({ "rule": "legacy" });
    let old = append(&root, legacy, "owner").expect("predecessor").id;
    let migrated = draft("agent.lesson.v2", "migrated", Some(old));

    append(&root, migrated, "owner").expect("migration over a keyless predecessor");
    append(&root, draft("agent.lesson.v2", "duplicate", None), "owner").expect_err("second head");
    assert_eq!(read_all(&root).expect("readable store").len(), 2);
    fs::remove_dir_all(root).expect("cleanup");
}

/// The candidate is a plain dag record and passes every rule its own type
/// declares. Only the graph it would create breaks the successor's linear
/// invariant, so the writer has to judge that graph or leave the store in a
/// state its own canonical read rejects.
#[test]
fn a_predecessor_that_only_the_full_graph_can_reject_is_refused_at_append() {
    let root = store("parity");
    register(&root, "agent.lesson.v1", LifecyclePolicy::default());
    register(&root, "agent.lesson.v2", linear(&["agent.lesson.v1"]));
    let old = append(&root, draft("agent.lesson.v1", "old", None), "owner")
        .expect("predecessor")
        .id;
    append(
        &root,
        draft("agent.lesson.v2", "migrated", Some(old)),
        "owner",
    )
    .expect("migration");

    let revived = append(&root, draft("agent.lesson.v1", "revived", None), "owner")
        .expect_err("a second head under the migrated key");

    assert!(revived.to_string().contains("multiple current heads"));
    assert_eq!(read_all(&root).expect("readable store").len(), 2);
    fs::remove_dir_all(root).expect("cleanup");
}
