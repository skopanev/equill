use super::support::*;
use crate::record::{RecordDraft, append};
use serde_json::json;
use std::fs;
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
