use super::{TypeDefinition, register};
use crate::command::init;
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);

fn store(name: &str) -> PathBuf {
    let suffix = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "equill-schema-{name}-{}-{suffix}",
        std::process::id()
    ));
    init::create(&path, "test-owner", "agent.memory").expect("initialize");
    path
}

fn definition(owner: &str) -> TypeDefinition {
    TypeDefinition {
        type_name: "agent.lesson.v1".into(),
        uri: "equill://agent.lesson/v1".into(),
        owner: owner.into(),
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

#[test]
fn old_definitions_default_to_dag_lifecycle() {
    let definition: TypeDefinition = serde_json::from_value(json!({
        "type": "agent.lesson.v1",
        "uri": "equill://agent.lesson/v1",
        "owner": "schema-owner",
        "payload_schema": { "type": "object" }
    }))
    .expect("legacy definition");

    assert_eq!(definition.lifecycle.mode, super::LifecycleMode::Dag);
    assert!(definition.lifecycle.allowed_predecessor_types.is_empty());
}

#[test]
fn linear_lifecycle_requires_key_pointer() {
    let path = store("linear-key");
    let mut item = definition("schema-owner");
    item.lifecycle.mode = super::LifecycleMode::Linear;

    let error = register(&path, item, "test-owner").expect_err("missing key pointer");

    assert!(error.to_string().contains("requires a JSON key_pointer"));
    fs::remove_dir_all(path).expect("remove test store");
}

#[test]
fn registration_is_immutable_and_idempotent() {
    let path = store("register");
    let first = register(&path, definition("schema-owner"), "test-owner").expect("register");
    let second = register(&path, definition("schema-owner"), "test-owner").expect("repeat");
    let conflict = register(&path, definition("other-owner"), "test-owner").expect_err("conflict");

    assert!(first.created);
    assert!(!second.created);
    assert_eq!(first.sha256, second.sha256);
    assert!(conflict.to_string().contains("registered differently"));
    fs::remove_dir_all(path).expect("remove test store");
}

#[test]
fn rejects_invalid_json_schema() {
    let path = store("invalid");
    let mut item = definition("schema-owner");
    item.payload_schema = json!({ "type": 42 });
    let error = register(&path, item, "test-owner").expect_err("invalid schema");

    assert!(error.to_string().contains("invalid schema"));
    fs::remove_dir_all(path).expect("remove test store");
}
