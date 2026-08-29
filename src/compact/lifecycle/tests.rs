use super::super::planner;
use crate::command::init;
use crate::schema::{self, LifecycleMode, LifecyclePolicy, TypeDefinition};
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

fn plan(name: &str, lifecycle: LifecyclePolicy, contents: String) -> (PathBuf, Result<(), String>) {
    let root = std::env::temp_dir().join(format!(
        "equill-compact-lifecycle-{name}-{}-{}",
        std::process::id(),
        Uuid::now_v7()
    ));
    init::create(&root, "test-owner", "agent.memory").expect("initialize");
    schema::register(
        &root,
        TypeDefinition {
            type_name: "agent.lesson.v1".into(),
            uri: "equill://agent.lesson/v1".into(),
            owner: "test-owner".into(),
            payload_schema: json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "required": ["key", "rule"],
                "additionalProperties": false,
                "properties": {
                    "key": { "type": "string" },
                    "rule": { "type": "string" }
                }
            }),
            lifecycle,
        },
        "test-owner",
    )
    .expect("register schema");
    let inputs = root.join("source");
    fs::create_dir(&inputs).expect("source directory");
    fs::write(inputs.join("source.jsonl"), contents).expect("source");
    let manifest = inputs.join("inputs.jsonl");
    fs::write(&manifest, "{\"path\":\"source.jsonl\"}\n").expect("manifest");
    let result = planner::build(
        &root,
        &manifest,
        "2026-08-27T00:00:00Z".parse().expect("time"),
    )
    .map(|_| ())
    .map_err(|error| error.to_string());
    (root, result)
}

fn line(id: &str, supersedes: Option<&str>) -> String {
    json!({
        "id": id,
        "ts": "2026-01-01T00:00:00Z",
        "namespace": "agent.memory",
        "type": "agent.lesson.v1",
        "actor": "legacy-writer",
        "observed_at": "2026-01-01T00:00:00Z",
        "payload": { "key": "shared", "rule": format!("Synthetic {id}") },
        "evidence": [],
        "tags": [],
        "supersedes": supersedes
    })
    .to_string()
        + "\n"
}

#[test]
fn dry_run_enforces_unknown_append_only_linear_and_dag_lifecycles() {
    let cases = [
        (
            "unknown",
            LifecyclePolicy::default(),
            line("root", Some("01a047d8-a818-7381-b809-2b96ae6bc6fa")),
            Some("unknown supersedes target"),
        ),
        (
            "append-only",
            LifecyclePolicy {
                mode: LifecycleMode::AppendOnly,
                ..LifecyclePolicy::default()
            },
            line("root", None) + &line("next", Some("root")),
            Some("append_only"),
        ),
        (
            "linear-fork",
            LifecyclePolicy {
                mode: LifecycleMode::Linear,
                key_pointer: Some("/key".into()),
                allowed_predecessor_types: Vec::new(),
            },
            line("root", None) + &line("left", Some("root")) + &line("right", Some("root")),
            Some("multiple current heads"),
        ),
        (
            "dag-fork",
            LifecyclePolicy::default(),
            line("root", None) + &line("left", Some("root")) + &line("right", Some("root")),
            None,
        ),
    ];
    for (name, lifecycle, contents, expected) in cases {
        let (root, result) = plan(name, lifecycle, contents);
        match expected {
            Some(needle) => {
                let error = result.expect_err("reject graph");
                assert!(error.contains(needle), "{name}: {error}");
            }
            None => result.expect("accept DAG fork"),
        }
        fs::remove_dir_all(root).expect("cleanup");
    }
}

/// Apply rebuilds the manifest into an empty shadow store, so a target that only
/// the live store holds is absent there. Plan and apply must therefore reach the
/// same verdict on it — and still agree when the manifest is self-contained.
#[test]
fn plan_and_apply_agree_on_a_target_outside_the_manifest() {
    let (root, result) = plan("external", LifecyclePolicy::default(), line("root", None));
    result.expect("baseline plan");
    let source = root.join("source/source.jsonl");
    let manifest = root.join("source/inputs.jsonl");
    crate::ingest::import_manifest(&root, &manifest, "test-owner").expect("first import");
    let live = crate::record::read_all(&root).expect("records")[0]
        .id
        .to_string();
    let external = line("root", None) + &line("next", Some(&live));
    fs::write(&source, &external).expect("source");

    let planned = super::super::run(&root, &manifest, false, "test-owner").expect_err("plan");
    let applied = super::super::run(&root, &manifest, true, "test-owner").expect_err("apply");

    for error in [&planned, &applied] {
        let error = error.to_string();
        assert!(error.contains("unknown supersedes target"), "{error}");
    }
    assert_eq!(crate::record::read_all(&root).expect("records").len(), 1);
    assert_eq!(fs::read_to_string(&source).expect("source"), external);

    // The same pair accepts the edge once the manifest carries its own target.
    fs::write(&source, line("root", None) + &line("next", Some("root"))).expect("source");
    super::super::run(&root, &manifest, false, "test-owner").expect("self-contained plan");
    super::super::run(&root, &manifest, true, "test-owner").expect("self-contained apply");
    fs::remove_dir_all(root).expect("cleanup");
}
