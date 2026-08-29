use super::{LifecycleMode, LifecyclePolicy, append, dag, draft, read_all, register, store};
use std::fs;
use uuid::Uuid;

fn append_only() -> LifecyclePolicy {
    LifecyclePolicy {
        mode: LifecycleMode::AppendOnly,
        ..LifecyclePolicy::default()
    }
}

#[test]
fn native_writer_rejects_unknown_target() {
    let root = store("unknown");
    register(&root, "agent.lesson.v1", LifecyclePolicy::default());

    let error = append(
        &root,
        draft("agent.lesson.v1", "unknown", Some(Uuid::now_v7())),
        "owner",
    )
    .expect_err("unknown target");

    assert!(error.to_string().contains("supersedes target is unknown"));
    assert!(read_all(&root).expect("records").is_empty());
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn append_only_rejects_replacement_while_dag_allows_branches() {
    let root = store("modes");
    register(&root, "agent.event.v1", append_only());
    register(&root, "agent.lesson.v1", LifecyclePolicy::default());
    let event = append(&root, draft("agent.event.v1", "event", None), "owner")
        .expect("event")
        .id;
    let event_error = append(
        &root,
        draft("agent.event.v1", "replacement", Some(event)),
        "owner",
    )
    .expect_err("append-only replacement");
    let lesson = append(&root, draft("agent.lesson.v1", "root", None), "owner")
        .expect("dag root")
        .id;
    for rule in ["left", "right"] {
        append(&root, draft("agent.lesson.v1", rule, Some(lesson)), "owner").expect("dag branch");
    }

    assert!(event_error.to_string().contains("append_only"));
    fs::remove_dir_all(root).expect("cleanup");
}

/// append_only is a claim about the record, not about who replaces it. A named
/// cross-type successor must not be an implicit way around the declaration.
#[test]
fn an_append_only_record_cannot_be_superseded_by_another_type() {
    let root = store("append-only-target");
    register(&root, "agent.event.v1", append_only());
    register(&root, "agent.lesson.v1", dag(&["agent.event.v1"]));
    let event = append(&root, draft("agent.event.v1", "event", None), "owner")
        .expect("event")
        .id;

    let error = append(
        &root,
        draft("agent.lesson.v1", "successor", Some(event)),
        "owner",
    )
    .expect_err("cross-type replacement of an append_only record");

    assert!(error.to_string().contains("cannot be superseded"));
    assert_eq!(read_all(&root).expect("readable store").len(), 1);
    fs::remove_dir_all(root).expect("cleanup");
}
