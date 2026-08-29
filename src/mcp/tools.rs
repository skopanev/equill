use crate::kernel::error::Error;
use crate::{context, filter, projection, record, schema, telemetry, vector};
use serde_json::{Value, json};
use std::path::Path;

/// The tool surface. Every entry maps onto an operation the CLI already calls,
/// so MCP stays an adapter: there is no second way into the store, and in
/// particular `record` goes through the same grant-checked immutable writer.
pub fn catalog() -> Value {
    json!({ "tools": [
        tool("status", "Report store health and installed components.", json!({ "type": "object", "properties": {} })),
        tool("schema_list", "List the record types this store has registered.", json!({ "type": "object", "properties": {} })),
        tool("schema_show", "Describe one type: fields, which are required, and any constrained vocabulary.",
            json!({ "type": "object", "required": ["type"], "properties": { "type": { "type": "string" } } })),
        tool("search", "Full-text search, optionally narrowed by schema-aware filters.",
            json!({ "type": "object", "required": ["query"], "properties": {
                "query": { "type": "string" },
                "namespace": { "type": "string" },
                "type": { "type": "string" },
                "limit": { "type": "integer", "minimum": 1, "maximum": 100 },
                "where": { "type": "array", "items": { "type": "string" } },
                "strict": { "type": "boolean" }
            }})),
        tool("context", "Assemble bounded context for a registered profile.",
            json!({ "type": "object", "required": ["profile"], "properties": {
                "profile": { "type": "string" },
                "query": { "type": "string" },
                "coordinates": { "type": "array", "items": { "type": "string" } },
                "tags": { "type": "array", "items": { "type": "string" } },
                "at": { "type": "string" },
                "include_superseded": { "type": "boolean" },
                "where": { "type": "array", "items": { "type": "string" } },
                "strict": { "type": "boolean" }
            }})),
        tool("get", "Read one record by id.",
            json!({ "type": "object", "required": ["id"], "properties": { "id": { "type": "string" } } })),
        tool("record", "Append one schema-validated immutable record through the canonical writer.",
            json!({ "type": "object", "required": ["draft"], "properties": { "draft": { "type": "object" } } })),
    ]})
}

/// Whether the server advertises this tool. Asking for anything else is a
/// malformed call rather than a failed one.
pub fn exists(name: &str) -> bool {
    catalog()["tools"]
        .as_array()
        .is_some_and(|tools| tools.iter().any(|tool| tool["name"] == name))
}

fn tool(name: &str, description: &str, schema: Value) -> Value {
    json!({ "name": name, "description": description, "inputSchema": schema })
}

pub fn call(
    store: &Path,
    actor: &str,
    log_queries: bool,
    name: &str,
    arguments: &Value,
) -> Result<Value, Error> {
    match name {
        "status" => value(&crate::command::status::report(Some(store))?),
        "schema_list" => value(&schema::list(store)?),
        "schema_show" => value(&schema::show(store, text(arguments, "type")?)?),
        "get" => {
            let id: uuid::Uuid = text(arguments, "id")?
                .parse()
                .map_err(|_| Error::InvalidRecord("id is not a record identifier".into()))?;
            let found = record::read_all(store)?
                .into_iter()
                .find(|item| item.id == id)
                .ok_or_else(|| Error::InvalidRecord(format!("no record with id {id}")))?;
            value(&found)
        }
        "search" => search(store, log_queries, arguments),
        "context" => assemble(store, actor, log_queries, arguments),
        "record" => {
            let draft = arguments
                .get("draft")
                .ok_or_else(|| Error::InvalidRecord("record needs a draft".into()))?;
            let draft: record::RecordDraft = serde_json::from_value(draft.clone())?;
            value(&record::append(store, draft, actor)?)
        }
        other => Err(Error::InvalidRecord(format!("unknown tool {other}"))),
    }
}

fn search(store: &Path, log_queries: bool, arguments: &Value) -> Result<Value, Error> {
    let filter = filter::Filter::parse(&strings(arguments, "where"), flag(arguments, "strict"))?;
    let type_name = optional(arguments, "type");
    filter::validate(&filter, &filter::in_scope(store, type_name.as_deref())?)?;
    let limit = arguments.get("limit").and_then(Value::as_u64).unwrap_or(20) as u16;
    let request = projection::SearchRequest {
        query: text(arguments, "query")?.to_owned(),
        namespace: optional(arguments, "namespace"),
        type_name,
        limit: filter::candidate_limit(record::read_all(store)?.len(), limit)?,
    };
    let mut report = vector::search(store, &request, vector::SearchStrategy::Fts)?;
    report
        .hits
        .retain(|hit| filter::matches(&hit.record, &filter));
    report.hits.truncate(limit as usize);
    // The same opt-in log the CLI writes: a miss rate that counted only the CLI
    // would measure the surface nobody uses once this becomes the main one.
    telemetry::record_query(
        store,
        "mcp.search",
        &request.query,
        Vec::new(),
        report.hits.len(),
        log_queries,
    );
    value(&report)
}

fn assemble(
    store: &Path,
    actor: &str,
    log_queries: bool,
    arguments: &Value,
) -> Result<Value, Error> {
    let filter = filter::Filter::parse(&strings(arguments, "where"), flag(arguments, "strict"))?;
    let request = context::inline_request(
        optional(arguments, "query"),
        strings(arguments, "coordinates"),
        strings(arguments, "tags"),
        Vec::new(),
        optional(arguments, "at"),
        flag(arguments, "include_superseded"),
    )?;
    let bundle = context::assemble(store, text(arguments, "profile")?, request, actor, &filter)?;
    telemetry::record_query(
        store,
        "mcp.context",
        &bundle.receipt.request_digest,
        bundle
            .receipt
            .unmatched_coordinates
            .iter()
            .map(|item| item.key.as_str())
            .collect(),
        bundle.selected_record_ids.len(),
        log_queries,
    );
    value(&bundle)
}

fn value<T: serde::Serialize>(report: &T) -> Result<Value, Error> {
    Ok(serde_json::to_value(report)?)
}

fn text<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, Error> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::InvalidRecord(format!("{key} is required")))
}

fn optional(arguments: &Value, key: &str) -> Option<String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn strings(arguments: &Value, key: &str) -> Vec<String> {
    arguments
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn flag(arguments: &Value, key: &str) -> bool {
    arguments.get(key).and_then(Value::as_bool).unwrap_or(false)
}
