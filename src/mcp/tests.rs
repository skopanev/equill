use super::serve;
use crate::command::init;
use crate::record::{RecordDraft, append};
use crate::schema::{self, TypeDefinition};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

fn store() -> PathBuf {
    let root = std::env::temp_dir().join(format!("equill-mcp-{}", Uuid::now_v7()));
    init::create(&root, "owner", "agent.memory").expect("initialize");
    schema::register(
        &root,
        TypeDefinition {
            type_name: "agent.lesson.v1".into(),
            uri: "equill://agent.lesson/v1".into(),
            owner: "owner".into(),
            payload_schema: json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": { "rule": { "type": "string" }, "source": { "type": "string" } },
                "required": ["rule"],
                "additionalProperties": false
            }),
            lifecycle: Default::default(),
        },
        "owner",
    )
    .expect("register schema");
    root
}

fn exchange(root: &Path, actor: &str, requests: &[Value]) -> Vec<Value> {
    exchange_logging(root, actor, false, requests)
}

fn exchange_logging(root: &Path, actor: &str, log_queries: bool, requests: &[Value]) -> Vec<Value> {
    let input = requests
        .iter()
        .map(|request| request.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let mut output = Vec::new();
    serve(root, actor, log_queries, input.as_bytes(), &mut output).expect("serve");
    String::from_utf8(output)
        .expect("utf8")
        .lines()
        .map(|line| serde_json::from_str(line).expect("json response"))
        .collect()
}

fn call(name: &str, arguments: Value, id: u64) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": { "name": name, "arguments": arguments } })
}

fn structured(response: &Value) -> &Value {
    &response["result"]["structuredContent"]
}

#[test]
fn the_handshake_advertises_every_tool_and_answers_ping() {
    let root = store();

    let replies = exchange(
        &root,
        "owner",
        &[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize",
                    "params": { "protocolVersion": "2025-06-18" } }),
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
            json!({ "jsonrpc": "2.0", "id": 3, "method": "ping" }),
            json!({ "jsonrpc": "2.0", "id": 4, "method": "nonsense" }),
        ],
    );

    // The notification is acted on but never answered, so four ids come back.
    assert_eq!(replies.len(), 4);
    assert_eq!(replies[0]["result"]["serverInfo"]["name"], "equill");
    // The version a client asked for comes back when we speak it.
    assert_eq!(replies[0]["result"]["protocolVersion"], "2025-06-18");
    let names = replies[1]["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|tool| tool["name"].as_str().expect("name").to_owned())
        .collect::<Vec<_>>();
    for expected in [
        "status",
        "schema_list",
        "schema_show",
        "search",
        "context",
        "get",
        "record",
    ] {
        assert!(
            names.contains(&expected.to_string()),
            "{expected} is offered"
        );
    }
    assert_eq!(replies[3]["error"]["code"], -32601);
    fs::remove_dir_all(root).expect("cleanup");
}

/// The adapter is a second surface, never a second write path: a record written
/// through MCP is the same immutable, grant-checked append, and an actor the
/// store does not allow is refused here exactly as it is at the CLI.
#[test]
fn writing_goes_through_the_canonical_writer_and_respects_grants() {
    let root = store();
    let draft = json!({
        "namespace": "agent.memory",
        "type": "agent.lesson.v1",
        "observed_at": "2026-01-01T00:00:00Z",
        "payload": { "rule": "Run the build checks", "source": "owner" }
    });

    let accepted = exchange(
        &root,
        "owner",
        &[call("record", json!({ "draft": draft }), 1)],
    );
    let refused = exchange(
        &root,
        "stranger",
        &[call(
            "record",
            json!({ "draft": {
                "namespace": "agent.memory",
                "type": "agent.lesson.v1",
                "observed_at": "2026-01-01T00:00:00Z",
                "payload": { "rule": "Should not be stored" }
            }}),
            2,
        )],
    );
    let invalid = exchange(
        &root,
        "owner",
        &[call(
            "record",
            json!({ "draft": {
                "namespace": "agent.memory",
                "type": "agent.lesson.v1",
                "observed_at": "2026-01-01T00:00:00Z",
                "payload": { "rule": 42 }
            }}),
            3,
        )],
    );

    assert_eq!(accepted[0]["result"]["isError"], false);
    // A refusal is a real answer, not a broken transport.
    assert!(refused[0]["error"].is_null());
    assert_eq!(refused[0]["result"]["isError"], true);
    assert_eq!(invalid[0]["result"]["isError"], true);
    assert_eq!(crate::record::read_all(&root).expect("records").len(), 1);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn reading_tools_answer_over_the_same_core_operations() {
    let root = store();
    let id = append(
        &root,
        RecordDraft {
            namespace: "agent.memory".into(),
            type_name: "agent.lesson.v1".into(),
            observed_at: "2026-01-01T00:00:00Z".into(),
            valid_at: None,
            payload: json!({ "rule": "Rotate credentials", "source": "owner" }),
            evidence: Vec::new(),
            tags: Vec::new(),
            supersedes: None,
        },
        "owner",
    )
    .expect("append")
    .id;

    let replies = exchange(
        &root,
        "owner",
        &[
            call("status", json!({}), 1),
            call("schema_list", json!({}), 2),
            call("schema_show", json!({ "type": "agent.lesson.v1" }), 3),
            call("get", json!({ "id": id.to_string() }), 4),
            call(
                "search",
                json!({ "query": "credentials", "where": ["source=owner"] }),
                5,
            ),
            call(
                "search",
                json!({ "query": "credentials", "where": ["sorce=owner"] }),
                6,
            ),
        ],
    );

    assert_eq!(structured(&replies[0])["store"]["initialized"], true);
    assert_eq!(
        structured(&replies[1])["types"][0]["type"],
        "agent.lesson.v1"
    );
    assert!(
        structured(&replies[2])["fields"]
            .as_array()
            .expect("fields")
            .len()
            >= 2
    );
    assert_eq!(structured(&replies[3])["id"], id.to_string());
    assert_eq!(
        structured(&replies[4])["hits"]
            .as_array()
            .expect("hits")
            .len(),
        1
    );
    // A typo names itself here too rather than returning an empty result.
    assert_eq!(replies[5]["result"]["isError"], true);
    assert!(
        replies[5]["result"]["content"][0]["text"]
            .as_str()
            .expect("text")
            .contains("sorce")
    );
    fs::remove_dir_all(root).expect("cleanup");
}

/// A server that answers with a fixed version regardless of the request looks
/// compatible without being it, and a malformed frame is not a question this
/// server can answer at all.
#[test]
fn the_protocol_is_negotiated_and_malformed_frames_are_refused() {
    let root = store();

    let replies = exchange(
        &root,
        "owner",
        &[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize",
                    "params": { "protocolVersion": "2025-11-25" } }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "initialize",
                    "params": { "protocolVersion": "1999-01-01" } }),
            json!({ "jsonrpc": "2.0", "id": 3, "method": "initialize" }),
            json!({ "id": 4, "method": "ping" }),
            json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/call",
                    "params": { "name": "no_such_tool", "arguments": {} } }),
            json!({ "jsonrpc": "2.0", "id": 6, "method": "tools/call", "params": {} }),
        ],
    );

    assert_eq!(replies[0]["result"]["protocolVersion"], "2025-11-25");
    // A version we do not speak gets our newest, not a pretence.
    assert_eq!(replies[1]["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(replies[2]["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(replies[3]["error"]["code"], -32600);
    // An unadvertised tool is a malformed call, not a tool that failed.
    assert_eq!(replies[4]["error"]["code"], -32602);
    assert!(
        replies[4]["error"]["message"]
            .as_str()
            .expect("text")
            .contains("no_such_tool")
    );
    assert_eq!(replies[5]["error"]["code"], -32602);
    fs::remove_dir_all(root).expect("cleanup");
}

/// If the adapter becomes the main way people ask questions, a miss rate that
/// counted only the CLI would measure the surface nobody uses.
#[test]
fn queries_through_the_adapter_reach_the_same_opt_in_log() {
    let root = store();

    // The same query twice: once with the log off, once on. Only the second
    // may leave a row, or the adapter would be writing without being asked.
    exchange(
        &root,
        "owner",
        &[call(
            "search",
            json!({ "query": "nothing-matches-this" }),
            1,
        )],
    );

    let replies = exchange_logging(
        &root,
        "owner",
        true,
        &[call(
            "search",
            json!({ "query": "nothing-matches-this" }),
            2,
        )],
    );
    let (total, missed) = crate::telemetry::misses(&root).expect("log");
    assert_eq!((total, missed), (1, 1));
    assert_eq!(replies[0]["result"]["isError"], false);
    let log = fs::read_to_string(root.join("diagnostics/queries.jsonl")).expect("log file");
    assert!(log.contains("mcp.search"), "{log}");
    fs::remove_dir_all(root).expect("cleanup");
}
