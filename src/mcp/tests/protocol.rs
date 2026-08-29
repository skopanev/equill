use super::support::*;
use serde_json::json;
use std::fs;
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
        "revoke",
    ] {
        assert!(
            names.contains(&expected.to_string()),
            "{expected} is offered"
        );
    }
    assert_eq!(replies[3]["error"]["code"], -32601);
    fs::remove_dir_all(root).expect("cleanup");
}

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
