use equill::record::{AppendRequest, RecordDraft};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn store() -> PathBuf {
    let root = std::env::temp_dir().join(format!("equill-keyed-process-{}", uuid::Uuid::now_v7()));
    equill::command::init::create(&root, "owner", "agent.memory").unwrap();
    equill::schema::register(&root, equill::schema::TypeDefinition {
        type_name: "agent.lesson.v1".into(), uri: "equill://agent.lesson/v1".into(), owner: "owner".into(),
        payload_schema: json!({"type":"object","properties":{"rule":{"type":"string"}},"required":["rule"]}), lifecycle: Default::default(),
    }, "owner").unwrap();
    root
}

fn draft() -> Value {
    json!({"namespace":"agent.memory","type":"agent.lesson.v1","observed_at":"2026-01-01T00:00:00Z","payload":{"rule":"synthetic retry"}})
}

fn cli(root: &Path, input: &Path, key: Option<&str>) -> Value {
    let mut command = Command::new(env!("CARGO_BIN_EXE_equill"));
    command
        .args(["record", "--json", "--store"])
        .arg(root)
        .arg("--input")
        .arg(input)
        .env("EQUILL_ACTOR", "owner");
    if let Some(key) = key {
        command.arg("--idempotency-key").arg(key);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn cli_process_restart_and_mcp_return_the_core_outcome() {
    let root = store();
    let input = root.join("draft.json");
    fs::write(&input, draft().to_string()).unwrap();
    let first = cli(&root, &input, Some("literal retry"));
    let second = cli(&root, &input, Some("literal retry"));
    assert_eq!(first["id"], second["id"]);
    assert_eq!(first["receipt"], second["receipt"]);
    let message = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"record","arguments":{"draft":draft(),"idempotency_key":"literal retry"}}});
    let incoming = format!("{message}\n");
    let mut outgoing = Vec::new();
    equill::mcp::serve(&root, "owner", false, incoming.as_bytes(), &mut outgoing).unwrap();
    let response: Value = serde_json::from_slice(&outgoing).unwrap();
    assert!(response["error"].is_null(), "{response}");
    assert!(response.to_string().contains(first["id"].as_str().unwrap()));
    let core = equill::record::append_request(
        &root,
        AppendRequest {
            draft: serde_json::from_value::<RecordDraft>(draft()).unwrap(),
            idempotency_key: Some("literal retry".into()),
        },
        "owner",
    )
    .unwrap();
    assert_eq!(core.id.to_string(), first["id"]);
    assert_eq!(equill::record::read_all(&root).unwrap().len(), 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn jsonl_entries_share_the_same_idempotency_contract() {
    let root = store();
    let input = root.join("batch.jsonl");
    let line = json!({"draft":draft(),"idempotency_key":"batch operation"});
    fs::write(&input, format!("{line}\n{line}\n")).unwrap();
    let first = cli(&root, &input, None);
    let second = cli(&root, &input, None);
    assert_eq!(first["records"][0]["id"], first["records"][1]["id"]);
    assert_eq!(first["records"], second["records"]);
    assert_eq!(equill::record::read_all(&root).unwrap().len(), 1);
    fs::remove_dir_all(root).unwrap();
}
