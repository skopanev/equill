use super::support::{Fixture, draft};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::io::Write;
use std::process::Stdio;

fn run(fixture: &Fixture, lines: &[Value], tail: &[u8]) -> Vec<Value> {
    let mut child = fixture
        .command()
        .args(["mcp", "--store"])
        .arg(&fixture.store)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("MCP");
    let mut input = child.stdin.take().expect("input");
    for line in lines {
        writeln!(input, "{line}").expect("request");
    }
    input.write_all(tail).expect("tail");
    drop(input);
    let output = child.wait_with_output().expect("MCP completion");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("UTF8")
        .lines()
        .map(|line| serde_json::from_str(line).expect("response"))
        .collect()
}

fn tool(id: usize, name: &str, arguments: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}})
}

#[test]
fn entire_mcp_catalog_success_protocol_and_execution_failures_are_audited_once() {
    let fixture = Fixture::new();
    let cases = [
        ("status", json!({})),
        ("schema_list", json!({})),
        ("schema_show", json!({"type":"agent.note.v1"})),
        (
            "search",
            json!({"query":"synthetic","vector_enabled":false}),
        ),
        ("context", json!({"profile":"reader","query":"synthetic"})),
        (
            "hook_context",
            json!({"hook_event_name":"PostToolBatch","profile":"reader","query":"synthetic"}),
        ),
        ("get", json!({"id":fixture.record})),
        ("record", json!({"draft":draft()})),
        ("revoke", json!({"id":fixture.record})),
    ];
    let mut requests = vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":3,"method":"ping"}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    ];
    for (index, (name, arguments)) in cases.iter().enumerate() {
        requests.push(tool(index + 10, name, arguments.clone()));
        let mut invalid = tool(index + 30, name, json!({}));
        invalid["jsonrpc"] = json!("invalid");
        requests.push(invalid);
    }
    for (index, (name, arguments)) in [
        ("get", json!({"id":"invalid"})),
        ("record", json!({"draft":{}})),
        (
            "context",
            json!({"profile":"missing","query":"private query"}),
        ),
        ("schema_show", json!({"type":"missing.type.v1"})),
    ]
    .into_iter()
    .enumerate()
    {
        requests.push(tool(index + 50, name, arguments));
    }
    let responses = run(&fixture, &requests, b"not JSON\n");
    let advertised: BTreeSet<_> =
        responses.iter().find(|r| r["id"] == 2).expect("catalog")["result"]["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .map(|tool| tool["name"].as_str().expect("name").to_owned())
            .collect();
    assert_eq!(
        advertised,
        cases.iter().map(|(name, _)| (*name).to_owned()).collect()
    );
    for index in 0..cases.len() {
        let success = responses
            .iter()
            .find(|r| r["id"] == index + 10)
            .expect("success");
        assert_eq!(success["result"]["isError"], false, "{success}");
        assert!(
            responses
                .iter()
                .find(|r| r["id"] == index + 30)
                .expect("failure")
                .get("error")
                .is_some()
        );
    }
    let events = fixture.events();
    assert_eq!(
        events.iter().filter(|event| event.surface == "mcp").count(),
        requests.len() + 1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.surface == "cli" && event.operation == "mcp")
            .count(),
        1
    );
    for (name, _) in cases {
        assert!(
            events
                .iter()
                .any(|e| e.operation == name && e.outcome == "success")
        );
        assert!(
            events
                .iter()
                .any(|e| e.operation == name && e.outcome == "error")
        );
    }
    let raw = serde_json::to_string(&events).expect("events");
    for secret in [
        "private query",
        "synthetic audit fixture",
        fixture.root.to_str().expect("path"),
    ] {
        assert!(!raw.contains(secret));
    }
}

#[test]
fn oversized_arguments_and_invalid_utf8_remain_bounded_and_payload_free() {
    let fixture = Fixture::new();
    let private = "never persist this request ".repeat(20_000);
    let response = run(
        &fixture,
        &[tool(1, "status", json!({"ignored":private}))],
        b"",
    );
    assert_eq!(response[0]["result"]["isError"], false);
    let events = fixture.events();
    let request = events
        .iter()
        .find(|event| event.surface == "mcp")
        .expect("request");
    assert!(request.arguments.bytes > 100_000);
    let json = serde_json::to_string(request).expect("event");
    assert!(json.len() < 8192 && !json.contains("never persist"));
    let before = events.len();
    let mut child = fixture
        .command()
        .args(["mcp", "--store"])
        .arg(&fixture.store)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("MCP");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(&[255, b'\n'])
        .expect("invalid input");
    assert!(!child.wait().expect("exit").success());
    let events = fixture.events();
    assert_eq!(
        events.len(),
        before + 2,
        "one transport failure and one CLI session failure"
    );
    assert!(
        events
            .iter()
            .any(|event| event.error_class.as_deref() == Some("transport"))
    );
}
