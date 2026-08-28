use super::{import_jsonl, import_manifest};
use crate::command::init;
use crate::schema;
use serde_json::json;
use std::fs;
use std::path::PathBuf;

fn target(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("equill-import-{name}-{}", std::process::id()))
}

#[test]
fn portable_schema_and_legacy_import_are_idempotent() {
    let root = target("single");
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

#[test]
fn manifest_imports_relative_inputs_and_doctor_verifies_the_set() {
    let root = target("manifest");
    let _ = fs::remove_dir_all(&root);
    init::create(&root, "test-owner", "agent.memory").expect("initialize");
    register_schema(&root);

    let inputs = root.join("source");
    fs::create_dir(&inputs).expect("create source directory");
    fs::write(
        inputs.join("one.jsonl"),
        legacy_line("lesson-1", "Keep evidence."),
    )
    .expect("write first input");
    fs::write(
        inputs.join("two.jsonl"),
        legacy_line("lesson-2", "Check the diff."),
    )
    .expect("write second input");
    let manifest = inputs.join("inputs.jsonl");
    fs::write(
        &manifest,
        "{\"path\":\"one.jsonl\",\"role\":\"rules\"}\n{\"path\":\"two.jsonl\",\"role\":\"lessons\"}\n",
    )
    .expect("write manifest");

    let first = import_manifest(&root, &manifest, "test-owner").expect("manifest import");
    let second = import_manifest(&root, &manifest, "test-owner").expect("repeat manifest");
    let scan = crate::integrity::scan(&root).expect("verify import receipts");

    assert_eq!((first.inputs, first.imported, first.skipped), (2, 2, 0));
    assert_eq!((second.inputs, second.imported, second.skipped), (2, 0, 2));
    assert_eq!(
        (scan.records, scan.import_receipts, scan.import_inputs),
        (2, 1, 2)
    );
    assert!(root.join(first.receipt).is_file());
    fs::remove_dir_all(root).expect("remove test store");
}

fn register_schema(root: &std::path::Path) {
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
    schema::register_file(root, &schema_path, "test-owner").expect("register schema");
}

fn legacy_line(id: &str, rule: &str) -> String {
    format!(
        "{}\n",
        serde_json::to_string(&json!({
            "id": id,
            "ts": "2026-01-01T12:00:00Z",
            "namespace": "agent.memory",
            "type": "agent.lesson.v1",
            "actor": "legacy-owner",
            "observed_at": "2026-01-01T12:00:00Z",
            "valid_at": "2026-01-01T12:00:00Z",
            "payload": { "rule": rule },
            "evidence": ["synthetic owner confirmation"],
            "tags": [],
            "supersedes": null
        }))
        .expect("serialize record")
    )
}

#[test]
fn supersedes_by_unknown_uuid_is_rejected_not_stored() {
    let root = target("dangling-supersedes");
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

    let line = |id: &str, supersedes: serde_json::Value| {
        json!({
            "id": id,
            "ts": "2026-01-01T12:00:00Z",
            "namespace": "agent.memory",
            "type": "agent.lesson.v1",
            "actor": "legacy-owner",
            "observed_at": "2026-01-01T12:00:00Z",
            "valid_at": "2026-01-01T12:00:00Z",
            "payload": { "rule": format!("rule for {id}") },
            "evidence": [],
            "tags": ["severity:must"],
            "supersedes": supersedes
        })
        .to_string()
    };

    // A uuid that parses but names no record in this store must not be taken on
    // faith: that is how a re-imported ledger loses every supersession quietly.
    let dangling = root.join("dangling.jsonl");
    fs::write(
        &dangling,
        line("legacy-2", json!("01a047d8-a818-7381-b809-2b96ae6bc6fa")) + "\n",
    )
    .expect("write dangling input");
    let error = import_jsonl(&root, &dangling, "test-owner")
        .expect_err("a supersedes target that does not exist must fail the import");
    assert!(error.to_string().contains("supersedes target is unknown"));

    // The legacy id of a record imported earlier in the same run still resolves.
    let chained = root.join("chained.jsonl");
    fs::write(
        &chained,
        format!(
            "{}\n{}\n",
            line("legacy-1", json!(null)),
            line("legacy-2", json!("legacy-1"))
        ),
    )
    .expect("write chained input");
    let report = import_jsonl(&root, &chained, "test-owner").expect("chained import");
    assert_eq!(report.imported, 2);

    let records = crate::record::read_all(&root).expect("read records");
    let ids: std::collections::HashSet<_> = records.iter().map(|record| record.id).collect();
    let targets: Vec<_> = records
        .iter()
        .filter_map(|record| record.supersedes)
        .collect();
    assert_eq!(targets.len(), 1);
    assert!(
        targets.iter().all(|target| ids.contains(target)),
        "every stored supersedes must point at a record that exists"
    );

    fs::remove_dir_all(&root).expect("remove store");
}
