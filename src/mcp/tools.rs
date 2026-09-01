use super::arguments::{flag, optional, strings, text, value};
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
        tool("search", "Search by text, by filter, or by both. Returns what is current.",
            json!({ "type": "object", "properties": {
                "query": { "type": "string" },
                "namespace": { "type": "string" },
                "type": { "type": "string" },
                "limit": { "type": "integer", "minimum": 1, "maximum": 100 },
                "where": { "type": "array", "items": { "type": "string" } },
                "strict": { "type": "boolean" }
            }})),
        tool("context", "Assemble bounded context from a profile: the one named here, or the one the store nominates.",
            json!({ "type": "object", "properties": {
                "profile": { "type": "string" },
                "project": { "type": "string" },
                "role": { "type": "string" },
                "phase": { "type": "string" },
                "harness": { "type": "string" },
                "process": { "type": "string" },
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
        tool("revoke", "Withdraw a record by writing a tombstone that supersedes it. Nothing is deleted.",
            json!({ "type": "object", "required": ["id"], "properties": {
                "id": { "type": "string" },
                "comment": { "type": "string" }
            }})),
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
        "revoke" => {
            let id: uuid::Uuid = text(arguments, "id")?
                .parse()
                .map_err(|_| Error::InvalidRecord("id is not a record identifier".into()))?;
            value(&record::revoke(
                store,
                id,
                optional(arguments, "comment").as_deref(),
                actor,
            )?)
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
    let query = optional(arguments, "query")
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty());
    if query.is_none() && filter.is_empty() {
        return Err(Error::Projection(
            "search needs a query, a where filter, or both".into(),
        ));
    }
    let namespace = optional(arguments, "namespace");
    // An unfiltered search reads one page; a filtered one must see the scope,
    // or its count would only ever describe the page it happened to get.
    let exhaustive = !filter.is_empty();
    let pool = if exhaustive {
        filter::candidate_limit(
            filter::scope_size(store, namespace.as_deref(), type_name.as_deref())?,
            limit,
        )?
    } else {
        limit
    };
    let request = projection::SearchRequest {
        query: query.clone(),
        namespace: namespace.clone(),
        type_name: type_name.clone(),
        // An unfiltered search has no reason to read past the page it was
        // asked for, so it does not pay for a full scan.
        limit: pool,
    };
    // Semantics by default, text when the request has to be complete. A filter
    // is settled after the search, so the search has to have seen everything it
    // could match — and an approximate-neighbour index returns near matches,
    // not every qualifying record. This is the CLI's `--all` rule reaching the
    // surface that has no `--all`: the promise is made by the filter instead.
    // A question gets both halves, even alongside a filter: the filter narrows
    // what may be returned, it does not turn the question into an enumeration.
    // A request with no question at all is an enumeration, and only text can
    // walk the scope it asks for.
    let strategy = if query.is_none() {
        vector::SearchStrategy::Fts
    } else {
        vector::SearchStrategy::Hybrid
    };
    let mut report = vector::search(store, &request, strategy)?;
    report
        .hits
        .retain(|hit| filter::matches(&hit.record, &filter));
    let matched = report.hits.len();
    report.hits.truncate(limit as usize);
    // The same settlement the CLI uses: a candidate pool that filled up is
    // never reported as an exact total, on either surface.
    vector::finalize(&mut report, matched, pool as usize, exhaustive);
    // The same opt-in log the CLI writes: a miss rate that counted only the CLI
    // would measure the surface nobody uses once this becomes the main one.
    telemetry::record_query(
        store,
        "mcp.search",
        request.query.as_deref().unwrap_or_default(),
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
    // Decided the same way as the CLI: the caller names a profile, or the
    // store does.
    let profile = match optional(arguments, "profile") {
        Some(named) => named,
        None => context::default_profile(store)?,
    };
    // The shorthands the CLI offers, spelled the same way here so the two
    // surfaces cannot drift into asking different questions.
    let mut coordinates = strings(arguments, "coordinates");
    for key in ["project", "role", "phase", "harness", "process"] {
        if let Some(value) = optional(arguments, key) {
            coordinates.push(format!("{key}={value}"));
        }
    }
    let request = context::inline_request(
        optional(arguments, "query"),
        coordinates,
        strings(arguments, "tags"),
        Vec::new(),
        optional(arguments, "at"),
        flag(arguments, "include_superseded"),
    )?;
    let bundle = context::assemble(store, &profile, request, actor, &filter)?;
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
