//! Process-boundary contract for `equill record --input`.
mod harness;

use harness::{binary, write_json};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn store(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "equill-record-input-{name}-{}-{}",
        std::process::id(),
        uuid::Uuid::now_v7().simple()
    ));
    let init = Command::new(binary())
        .args(["init", "--owner", "owner", "--namespace", "agent.memory"])
        .arg("--store")
        .arg(&root)
        .output()
        .expect("init process");
    assert!(init.status.success(), "init failed: {}", stderr(&init));
    let schema = root.join("schema.json");
    write_json(
        &schema,
        &json!({
            "type": "agent.lesson.v1",
            "uri": "equill://agent.lesson/v1",
            "owner": "owner",
            "payload_schema": {
                "type": "object",
                "properties": { "rule": { "type": "string" } },
                "required": ["rule"],
                "additionalProperties": false
            }
        }),
    );
    let register = Command::new(binary())
        .args(["schema", "register", "--file"])
        .arg(schema)
        .arg("--store")
        .arg(&root)
        .env("EQUILL_ACTOR", "owner")
        .output()
        .expect("schema process");
    assert!(
        register.status.success(),
        "schema failed: {}",
        stderr(&register)
    );
    root
}

fn draft(rule: Value) -> Value {
    json!({
        "namespace": "agent.memory",
        "type": "agent.lesson.v1",
        "observed_at": "2026-01-01T00:00:00Z",
        "payload": { "rule": rule }
    })
}

fn record(root: &Path, input: &Path) -> Output {
    Command::new(binary())
        .args(["record", "--json", "--input"])
        .arg(input)
        .arg("--store")
        .arg(root)
        .env("EQUILL_ACTOR", "owner")
        .output()
        .expect("record process")
}

fn body(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("JSON stdout")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn pretty_printed_object_is_one_successful_record() {
    let root = store("pretty");
    let input = root.join("pretty.json");
    write_json(&input, &draft(json!("Keep one draft whole.")));

    let output = record(&root, &input);

    assert!(
        output.status.success(),
        "record failed: {}",
        stderr(&output)
    );
    let receipt = body(&output);
    assert_eq!(receipt["ok"], true);
    assert!(receipt["id"].is_string());
    assert!(receipt["receipt"].is_string());
    assert_eq!(equill::record::read_all(&root).expect("ledger").len(), 1);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn malformed_object_exits_nonzero_and_stores_nothing() {
    let root = store("malformed");
    let input = root.join("malformed.json");
    fs::write(&input, "{\n  \"namespace\": \"agent.memory\",\n").expect("input");

    let output = record(&root, &input);

    assert!(!output.status.success(), "malformed input exited zero");
    assert!(
        output.stdout.is_empty(),
        "unexpected stdout: {:?}",
        output.stdout
    );
    assert!(stderr(&output).contains("invalid JSON"));
    assert!(equill::record::read_all(&root).expect("ledger").is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn schema_rejection_exits_nonzero_and_stores_nothing() {
    let root = store("schema-rejection");
    let input = root.join("rejected.json");
    write_json(&input, &draft(json!(42)));

    let output = record(&root, &input);

    assert!(!output.status.success(), "schema rejection exited zero");
    assert!(
        output.stdout.is_empty(),
        "unexpected stdout: {:?}",
        output.stdout
    );
    assert!(
        stderr(&output).contains("does not match"),
        "{}",
        stderr(&output)
    );
    assert!(equill::record::read_all(&root).expect("ledger").is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn rejected_jsonl_batch_keeps_json_receipt_and_exits_nonzero() {
    let root = store("batch-rejection");
    let input = root.join("batch.jsonl");
    let lines = format!("{}\n{}\n", draft(json!("Stored.")), draft(json!(42)));
    fs::write(&input, lines).expect("input");

    let output = record(&root, &input);

    assert!(!output.status.success(), "rejected batch exited zero");
    assert!(
        output.stderr.is_empty(),
        "unexpected stderr: {}",
        stderr(&output)
    );
    let receipt = body(&output);
    assert_eq!(receipt["ok"], false);
    assert_eq!(receipt["stored"], 1);
    assert_eq!(receipt["rejected"], 1);
    assert!(receipt["records"][0]["id"].is_string());
    assert!(receipt["records"][1]["error"].is_string());
    assert_eq!(equill::record::read_all(&root).expect("ledger").len(), 1);

    let rejected = root.join("all-rejected.jsonl");
    let lines = format!("{}\n{}\n", draft(json!(41)), draft(json!(42)));
    fs::write(&rejected, lines).expect("rejected input");
    let output = record(&root, &rejected);
    assert!(!output.status.success(), "zero-stored batch exited zero");
    let receipt = body(&output);
    assert_eq!(receipt["ok"], false);
    assert_eq!(receipt["stored"], 0);
    assert_eq!(receipt["rejected"], 2);
    assert_eq!(receipt["records"].as_array().expect("items").len(), 2);
    assert_eq!(equill::record::read_all(&root).expect("ledger").len(), 1);
    let _ = fs::remove_dir_all(root);
}
