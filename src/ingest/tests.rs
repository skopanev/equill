use super::import_jsonl;
use crate::command::init;
use crate::schema;
use serde_json::json;
use std::fs;
use std::path::PathBuf;

fn target() -> PathBuf {
    std::env::temp_dir().join(format!("equill-import-test-{}", std::process::id()))
}

#[test]
fn portable_schema_and_legacy_import_are_idempotent() {
    let root = target();
    let _ = fs::remove_dir_all(&root);
    init::create(&root, "test-owner", "agent.memory").expect("initialize");

    let schema_path = root.join("portable-schema.json");
    fs::write(
        &schema_path,
        serde_json::to_vec(&json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": "equill://agent.lesson/v1",
            "type": "object",
            "required": ["rule"],
            "additionalProperties": false,
            "properties": { "rule": { "type": "string" } },
            "x-equill-envelope": {
                "namespace": "agent.memory",
                "type": "agent.lesson.v1"
            }
        }))
        .expect("serialize schema"),
    )
    .expect("write schema");
    schema::register_file(&root, &schema_path, "test-owner").expect("register portable schema");

    let input = root.join("legacy.jsonl");
    let source = json!({
        "id": "legacy-lesson-1",
        "ts": "2026-01-01T12:00:00Z",
        "namespace": "agent.memory",
        "type": "agent.lesson.v1",
        "actor": "legacy-owner",
        "observed_at": "2026-01-01T12:00:00Z",
        "valid_at": "2026-01-01T12:00:00Z",
        "payload": { "rule": "Verify the focused change." },
        "evidence": ["synthetic owner confirmation"],
        "tags": ["verification"],
        "supersedes": null
    });
    fs::write(
        &input,
        format!(
            "{}\n",
            serde_json::to_string(&source).expect("serialize record")
        ),
    )
    .expect("write legacy JSONL");

    let first = import_jsonl(&root, &input, "test-owner").expect("first import");
    let second = import_jsonl(&root, &input, "test-owner").expect("repeat import");

    assert_eq!((first.imported, first.skipped), (1, 0));
    assert_eq!((second.imported, second.skipped), (0, 1));
    assert_eq!(crate::integrity::scan(&root).expect("verify").records, 1);
    fs::remove_dir_all(root).expect("remove test store");
}
