use super::super::serve;
use crate::command::init;
use crate::schema::{self, TypeDefinition};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub fn store() -> PathBuf {
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

pub fn exchange(root: &Path, actor: &str, requests: &[Value]) -> Vec<Value> {
    exchange_logging(root, actor, false, requests)
}

pub fn exchange_logging(
    root: &Path,
    actor: &str,
    log_queries: bool,
    requests: &[Value],
) -> Vec<Value> {
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

pub fn call(name: &str, arguments: Value, id: u64) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": { "name": name, "arguments": arguments } })
}

pub fn structured(response: &Value) -> &Value {
    &response["result"]["structuredContent"]
}
