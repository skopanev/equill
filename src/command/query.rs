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
    );
    command::output::render(json, &bundle, text)
}

#[allow(clippy::too_many_arguments)]
pub fn search(
    json: bool,
    store: PathBuf,
    query: String,
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
    // The projection caps its own result set, so a filter that runs
    // afterwards must inspect the entire corpus or refuse explicitly.
    let pool = if filter.is_empty() {
        limit
    } else {
        filter::candidate_limit(record::read_all(&store)?.len(), limit)?
    };
    let report_query = query.clone();
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
    let text = match &report.fallback {
        Some(reason) => format!(
            "{} hits via {} (vector unavailable: {reason})",
            report.hits.len(),
            report.answered_by
        ),
        None => format!("{} hits via {}", report.hits.len(), report.answered_by),
    };
    let text = if fields.is_empty() && matches!(format, command::cli::FormatArg::Jsonl) {
        text
    } else {
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
    );
    command::output::render(json, &report, text)
}

pub fn shape(format: crate::command::cli::FormatArg) -> crate::command::present::Format {
    match format {
        crate::command::cli::FormatArg::Jsonl => crate::command::present::Format::Jsonl,
        crate::command::cli::FormatArg::Text => crate::command::present::Format::Text,
    }
}
