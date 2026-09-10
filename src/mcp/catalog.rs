use serde_json::{Value, json};

/// Every MCP entry maps onto the same core operation the CLI calls.
pub fn catalog() -> Value {
    json!({ "tools": [
        tool("status", "Report store health and installed components.", empty()),
        tool("schema_list", "List registered record types.", empty()),
        tool("schema_show", "Describe one registered type.",
            json!({ "type": "object", "required": ["type"],
                "properties": { "type": { "type": "string" } } })),
        tool("search", "Search by text, filter, or both.",
            json!({ "type": "object", "properties": {
                "query": { "type": "string" },
                "namespace": { "type": "string" },
                "type": { "type": "string" },
                "limit": { "type": "integer", "minimum": 1, "maximum": 100 },
                "where": strings(),
                "strict": { "type": "boolean" },
                "query_instruction": { "type": "string" },
                "vector_enabled": { "type": "boolean" },
                "vector_score_threshold": { "type": "number", "minimum": -1, "maximum": 1 },
                "hybrid_order": sources(),
                "hybrid_fill_remaining": { "type": "boolean" },
                "hybrid_deduplicate": { "type": "boolean" }
            }})),
        tool("context", "Assemble bounded context from a profile.", context_schema()),
        tool("hook_context", "Assemble context for an editor lifecycle hook.",
            hook_schema()),
        tool("get", "Read one record by id.", required("id")),
        tool("revoke", "Withdraw a record by writing a tombstone.",
            json!({ "type": "object", "required": ["id"], "properties": {
                "id": { "type": "string" }, "comment": { "type": "string" }
            }})),
        tool("record", "Append one validated immutable record.",
            json!({ "type": "object", "required": ["draft"],
                "properties": { "draft": { "type": "object" }, "idempotency_key": { "type": "string" } } })),
    ]})
}

pub fn exists(name: &str) -> bool {
    catalog()["tools"]
        .as_array()
        .is_some_and(|tools| tools.iter().any(|tool| tool["name"] == name))
}

/// Everything the `context` tool accepts. Named once, because the hook takes
/// the same arguments and a second copy of the list would drift from this one.
fn context_schema() -> Value {
    json!({ "type": "object", "properties": {
        "profile": { "type": "string" },
        "project": { "type": "string" },
        "role": { "type": "string" },
        "phase": { "type": "string" },
        "harness": { "type": "string" },
        "process": { "type": "string" },
        "query": { "type": "string" },
        "coordinates": strings(),
        "tags": strings(),
        "at": { "type": "string" },
        "include_superseded": { "type": "boolean" },
        "budget": { "type": "integer", "minimum": 1 },
        "budget_records": { "type": "integer", "minimum": 1 },
        "where": strings(),
        "strict": { "type": "boolean" },
        "query_instruction": { "type": "string" },
        "vector_enabled": { "type": "boolean" },
        "vector_score_threshold": { "type": "number", "minimum": -1, "maximum": 1 },
        "hybrid_order": sources(),
        "hybrid_fill_remaining": { "type": "boolean" },
        "hybrid_deduplicate": { "type": "boolean" }
    }})
}

/// The same, plus the event being answered. The hook adds no other argument:
/// the launcher supplies the question as `query`, exactly as `context` takes it.
fn hook_schema() -> Value {
    let mut schema = context_schema();
    schema["required"] = json!(["hook_event_name"]);
    schema["properties"]["hook_event_name"] =
        json!({ "type": "string", "enum": ["UserPromptSubmit", "PostToolBatch", "PostToolUse"] });
    schema
}

fn tool(name: &str, description: &str, schema: Value) -> Value {
    json!({ "name": name, "description": description, "inputSchema": schema })
}

fn empty() -> Value {
    json!({ "type": "object", "properties": {} })
}

fn required(name: &str) -> Value {
    json!({ "type": "object", "required": [name],
        "properties": { (name): { "type": "string" } } })
}

fn strings() -> Value {
    json!({ "type": "array", "items": { "type": "string" } })
}

fn sources() -> Value {
    json!({ "type": "array", "minItems": 2, "maxItems": 2,
        "items": { "type": "string", "enum": ["vector", "fts"] } })
}
