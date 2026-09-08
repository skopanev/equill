use super::arguments::{flag, optional, strings, text, value};
use super::environment;
use crate::kernel::error::Error;
use crate::{context, filter, projection, record, schema, telemetry, vector};
use serde_json::Value;
use std::path::Path;

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
    let started = std::time::Instant::now();
    let policy = crate::retrieval::resolve(store, super::retrieval::overrides(arguments)?)?;
    let filter = filter::Filter::parse(&strings(arguments, "where"), flag(arguments, "strict"))?;
    let type_name = optional(arguments, "type");
    filter::validate(&filter, &filter::in_scope(store, type_name.as_deref())?)?;
    let limit = arguments
        .get("limit")
        .and_then(Value::as_u64)
        .or_else(|| policy.default_budget_records.map(|value| value as u64))
        .unwrap_or(20) as u16;
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
    let mut report = vector::search_with_policy(store, &request, strategy, &policy)?;
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
        telemetry::QueryOutcome {
            coordinates: Vec::new(),
            results: report.hits.len(),
            elapsed_ms: telemetry::elapsed_ms(started),
            request_digest: None,
            receipt_path: None,
        },
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
    let started = std::time::Instant::now();
    let retrieval = super::retrieval::overrides(arguments)?;
    let filter = filter::Filter::parse(&strings(arguments, "where"), flag(arguments, "strict"))?;
    // Decided the same way as the CLI: the caller names a profile, or the
    // store does.
    let profile = match optional(arguments, "profile") {
        Some(named) => named,
        None => context::default_profile(store)?,
    };
    let coordinates = environment::context_coordinates(arguments);
    let request = context::inline_request(
        optional(arguments, "query"),
        coordinates,
        strings(arguments, "tags"),
        Vec::new(),
        optional(arguments, "at"),
        flag(arguments, "include_superseded"),
    )?;
    let raw_query = request.query.clone();
    let runtime_budget_tokens = positive_usize(arguments, "budget")?;
    let runtime_budget_records = positive_usize(arguments, "budget_records")?;
    let bundle = context::assemble_with_options(
        store,
        &profile,
        request,
        actor,
        &filter,
        context::RuntimeBudget {
            tokens: runtime_budget_tokens,
            records: runtime_budget_records,
        },
        retrieval,
    )?;
    telemetry::record_query(
        store,
        "mcp.context",
        &raw_query,
        telemetry::QueryOutcome {
            coordinates: bundle
                .receipt
                .unmatched_coordinates
                .iter()
                .map(|item| item.key.as_str())
                .collect(),
            results: bundle.selected_record_ids.len(),
            elapsed_ms: telemetry::elapsed_ms(started),
            request_digest: Some(&bundle.receipt.request_digest),
            receipt_path: bundle.receipt_path.as_deref(),
        },
        log_queries,
    );
    value(&bundle)
}

fn positive_usize(arguments: &Value, key: &str) -> Result<Option<usize>, Error> {
    let Some(value) = arguments.get(key) else {
        return Ok(None);
    };
    let value = value
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| Error::Context(format!("{key} must be a positive integer")))?;
    Ok(Some(value))
}
