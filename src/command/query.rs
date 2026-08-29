//! The two retrieval surfaces. They are long because each one parses a filter,
//! decides a shape and records its own outcome; keeping them here leaves the
//! command list in `lib.rs` readable as a list.
use crate::kernel::error::Error;
use crate::{command, context, filter, kernel, projection, record, telemetry, vector};
use std::path::PathBuf;

#[allow(clippy::too_many_arguments)]
pub fn context(
    json: bool,
    store: PathBuf,
    profile: String,
    request: Option<PathBuf>,
    query: Option<String>,
    mut coordinates: Vec<String>,
    project: Option<String>,
    role: Option<String>,
    phase: Option<String>,
    harness: Option<String>,
    tags: Vec<String>,
    kinds: Vec<String>,
    at: Option<String>,
    include_superseded: bool,
    filters: Vec<String>,
    strict: bool,
    format: command::cli::FormatArg,
    fields: Vec<String>,
) -> Result<String, Error> {
    let actor = kernel::identity::actor_from_env()?;
    let filter = filter::Filter::parse(&filters, strict)?;
    for (key, value) in [
        ("project", project),
        ("role", role),
        ("phase", phase),
        ("harness", harness),
    ] {
        if let Some(value) = value {
            coordinates.push(format!("{key}={value}"));
        }
    }
    let bundle = match request {
        Some(path) => context::assemble_file(&store, &profile, &path, &actor, &filter)?,
        None => {
            let request =
                context::inline_request(query, coordinates, tags, kinds, at, include_superseded)?;
            context::assemble(&store, &profile, request, &actor, &filter)?
        }
    };
    let text = if fields.is_empty() && matches!(format, command::cli::FormatArg::Jsonl) {
        bundle.content.clone()
    } else {
        let selected = record::read_all(&store)?
            .into_iter()
            .filter(|item| bundle.selected_record_ids.contains(&item.id))
            .collect::<Vec<_>>();
        command::present::records(&selected, shape(format), &fields)?
    };
    telemetry::record_query(
        &store,
        "context",
        &bundle.receipt.request_digest,
        bundle
            .receipt
            .unmatched_coordinates
            .iter()
            .map(|item| item.key.as_str())
            .collect(),
        bundle.selected_record_ids.len(),
        telemetry::enabled(),
    );
    command::output::render(json, &bundle, text)
}

#[allow(clippy::too_many_arguments)]
pub fn search(
    json: bool,
    store: PathBuf,
    query: Option<String>,
    namespace: Option<String>,
    type_name: Option<String>,
    limit: u16,
    strategy: command::cli::StrategyArg,
    filters: Vec<String>,
    strict: bool,
    format: command::cli::FormatArg,
    fields: Vec<String>,
) -> Result<String, Error> {
    let filter = filter::Filter::parse(&filters, strict)?;
    filter::validate(&filter, &filter::in_scope(&store, type_name.as_deref())?)?;
    // A filter can fully determine a result set, so text is optional when one
    // is given. With neither, there is no question to answer — and answering
    // an empty query with everything would be a wildcard nobody asked for.
    let query = match query.as_deref().map(str::trim) {
        Some(text) if !text.is_empty() => Some(text.to_owned()),
        _ if !filter.is_empty() => None,
        _ => {
            return Err(Error::Projection(
                "search needs a --query, a --where filter, or both".into(),
            ));
        }
    };
    // The projection caps its own result set, so a filter that runs
    // afterwards must inspect the entire corpus or refuse explicitly.
    let pool = if filter.is_empty() {
        limit
    } else {
        filter::candidate_limit(
            filter::scope_size(&store, namespace.as_deref(), type_name.as_deref())?,
            limit,
        )?
    };
    let report_query = query.clone().unwrap_or_default();
    let request = projection::SearchRequest {
        query,
        namespace,
        type_name,
        limit: pool,
    };
    let strategy = match strategy {
        command::cli::StrategyArg::Fts => vector::SearchStrategy::Fts,
        command::cli::StrategyArg::Vector => vector::SearchStrategy::Vector,
        command::cli::StrategyArg::Hybrid => vector::SearchStrategy::Hybrid,
    };
    let mut report = vector::search(&store, &request, strategy)?;
    report
        .hits
        .retain(|hit| filter::matches(&hit.record, &filter));
    report.hits.truncate(limit as usize);
    // The fallback reason is not lost: it stays in the report, which --json
    // prints in full. What a result set must not carry is a summary sentence
    // mixed into the records themselves.
    // jsonl is a record stream, never a sentence: a caller piping it should not
    // have to strip a summary line first.
    let text = {
        let hits = report
            .hits
            .iter()
            .map(|hit| hit.record.clone())
            .collect::<Vec<_>>();
        command::present::records(&hits, shape(format), &fields)?
    };
    telemetry::record_query(
        &store,
        "search",
        &report_query,
        Vec::new(),
        report.hits.len(),
        telemetry::enabled(),
    );
    command::output::render(json, &report, text)
}

pub fn shape(format: crate::command::cli::FormatArg) -> crate::command::present::Format {
    match format {
        crate::command::cli::FormatArg::Jsonl => crate::command::present::Format::Jsonl,
        crate::command::cli::FormatArg::Text => crate::command::present::Format::Text,
    }
}

/// Withdrawing a record is a write, so it reads the actor the same way every
/// other write does.
pub fn revoke(
    json: bool,
    store: &std::path::Path,
    id: &str,
    comment: Option<&str>,
) -> Result<String, Error> {
    let actor = crate::kernel::identity::actor_from_env()?;
    let id: uuid::Uuid = id
        .parse()
        .map_err(|_| Error::InvalidRecord(format!("{id} is not an id")))?;
    let report = record::revoke(store, id, comment, &actor)?;
    let text = format!(
        "Revoked {} — tombstone {}",
        report.revoked, report.tombstone
    );
    command::output::render(json, &report, text)
}

pub fn get(
    json: bool,
    store: &std::path::Path,
    id: &str,
    format: command::cli::FormatArg,
    fields: &[String],
) -> Result<String, Error> {
    let id: uuid::Uuid = id
        .parse()
        .map_err(|_| Error::InvalidRecord(format!("{id} is not an id")))?;
    let found = record::read_all(store)?
        .into_iter()
        .find(|record| record.id == id)
        .ok_or_else(|| Error::InvalidRecord(format!("no record with id {id}")))?;
    let text = command::present::records(std::slice::from_ref(&found), shape(format), fields)?;
    command::output::render(json, &found, text)
}
