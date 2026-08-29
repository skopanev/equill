use super::{
    LifecyclePolicy, append, draft, lifecycle, linear, read_all, register, schema, store, stored,
};
use std::fs;

#[test]
fn cross_type_requires_explicit_predecessor() {
    let root = store("migration");
    register(&root, "agent.lesson.v1", LifecyclePolicy::default());
    register(&root, "agent.other.v1", linear(&[]));
    register(&root, "agent.lesson.v2", linear(&["agent.lesson.v1"]));
    let old = append(&root, draft("agent.lesson.v1", "old", None), "owner")
        .expect("old")
        .id;

    let rejected = append(
        &root,
        draft("agent.other.v1", "wrong type", Some(old)),
        "owner",
    )
    .expect_err("implicit cross-type replacement");
    append(
        &root,
        draft("agent.lesson.v2", "migrated", Some(old)),
        "owner",
    )
    .expect("explicit migration");

    assert!(
        rejected
            .to_string()
            .contains("cannot supersede predecessor type")
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn graph_rejects_self_and_namespace_boundary() {
    let root = store("graph");
    register(&root, "agent.lesson.v1", LifecyclePolicy::default());
    let mut self_cycle = stored("agent.memory", "agent.lesson.v1", None);
    self_cycle.supersedes = Some(self_cycle.id);
    let definition = schema::load(&root, "agent.lesson.v1").expect("schema");
    let self_error = lifecycle::validate_graph(&root, &[self_cycle]).expect_err("self cycle");

    let target = stored("other.memory", "agent.lesson.v1", None);
    let candidate = stored("agent.memory", "agent.lesson.v1", Some(target.id));
    let namespace_error = lifecycle::validate_append(&root, vec![target], &candidate, &definition)
        .expect_err("namespace boundary");

    assert!(self_error.to_string().contains("supersedes itself"));
    assert!(namespace_error.to_string().contains("cross namespaces"));
    assert!(read_all(&root).expect("readable store").is_empty());
    fs::remove_dir_all(root).expect("cleanup");
}
