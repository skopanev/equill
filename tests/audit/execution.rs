//! Actual governance/import dispatch, not parser help masquerading as execution.
use super::support::{Fixture, write};
use serde_json::json;
use std::process::Output;

fn checked(fixture: &Fixture, operation: &str, success: bool, run: impl FnOnce() -> Output) {
    let before = fixture
        .events()
        .iter()
        .filter(|event| event.operation == operation)
        .count();
    let output = run();
    assert_eq!(
        output.status.success(),
        success,
        "{operation}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let after = fixture.events();
    let matching: Vec<_> = after
        .iter()
        .filter(|event| event.operation == operation)
        .collect();
    assert_eq!(matching.len(), before + 1, "{operation}");
    let event = matching.last().expect("terminal event");
    assert_eq!(event.operation, operation);
    assert_eq!(event.outcome, if success { "success" } else { "error" });
    assert_eq!(event.domain_outcome, event.outcome);
}

#[test]
fn init_import_and_governance_execute_and_refuse_once() {
    let fixture = Fixture::new();
    let new_store = fixture.root.join("second-memory");
    let mut args = [
        "init",
        "--store",
        new_store.to_str().expect("path"),
        "--owner",
        "owner",
        "--namespace",
        "agent.memory",
    ];
    checked(&fixture, "init", true, || fixture.cli(&args));
    args[4] = "different-owner";
    checked(&fixture, "init", false, || fixture.cli(&args));
    let legacy = fixture.root.join("legacy.jsonl");
    std::fs::write(
        &legacy,
        format!(
            "{}\n",
            json!({
                "id":"synthetic-source-1", "ts":"2026-01-01T00:00:00Z",
                "namespace":"agent.memory", "type":"agent.note.v1", "actor":"source-writer",
                "observed_at":"2026-01-01T00:00:00Z", "payload":{"text":"synthetic imported note"}
            })
        ),
    )
    .expect("legacy source");
    let cases = [
        (
            "import",
            vec!["import", "--input", legacy.to_str().expect("path")],
        ),
        (
            "grant.add",
            vec![
                "grant",
                "add",
                "--actor",
                "writer",
                "--namespace",
                "agent.memory",
                "--types",
                "agent.note.v1",
            ],
        ),
        ("grant.revoke", vec!["grant", "revoke", "--actor", "writer"]),
        ("reader.add", vec!["reader", "add", "--actor", "reader"]),
        (
            "reader.revoke",
            vec!["reader", "revoke", "--actor", "reader"],
        ),
        (
            "owner.transfer",
            vec!["owner", "transfer", "--to", "next-owner"],
        ),
    ];
    for (operation, args) in cases {
        // A fully parsed request reaches a real authorization boundary first.
        checked(&fixture, operation, false, || {
            fixture
                .command()
                .env("EQUILL_ACTOR", "outsider")
                .args(&args)
                .arg("--store")
                .arg(&fixture.store)
                .output()
                .expect("refusal")
        });
        checked(&fixture, operation, true, || fixture.scoped(&args));
    }
}

#[test]
fn vector_governance_and_disabled_execution_have_terminal_events_without_network() {
    let fixture = Fixture::new();
    let config = fixture.root.join("vector.json");
    write(
        &config,
        &json!({
            "schema":"equill.qdrant-config.v1", "enabled":false,
            "endpoint":"http://127.0.0.1:1", "collection_alias":"synthetic_audit",
            "store_id":uuid::Uuid::now_v7(), "dimensions":4096, "distance":"cosine",
            "embedding":{
                "provider":"ollama", "endpoint":"http://127.0.0.1:1",
                "model_id":"qwen3-embedding:8b-q8_0", "model_sha256":"a".repeat(64),
                "input_schema":"equill.record.embedding.v1"
            }
        }),
    );
    for (operation, args) in [
        (
            "vector.configure",
            vec![
                "vector",
                "configure",
                "--file",
                config.to_str().expect("path"),
            ],
        ),
        ("vector.disable", vec!["vector", "disable"]),
    ] {
        checked(&fixture, operation, false, || {
            fixture
                .command()
                .env("EQUILL_ACTOR", "outsider")
                .args(&args)
                .arg("--store")
                .arg(&fixture.store)
                .output()
                .expect("refusal")
        });
        checked(&fixture, operation, true, || fixture.scoped(&args));
    }
    for operation in ["sync", "rebuild"] {
        checked(&fixture, &format!("vector.{operation}"), false, || {
            fixture.scoped(&["vector", operation])
        });
    }
    checked(&fixture, "vector.drain", false, || {
        fixture.scoped(&["vector", "drain", "--once"])
    });
}
