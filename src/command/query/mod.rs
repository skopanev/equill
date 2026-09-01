//! The retrieval surfaces. Each parses a filter, decides a shape and records
//! its own outcome; keeping them here leaves the command list in `lib.rs`
//! readable as a list.
mod context;

pub use context::context;

use crate::kernel::error::Error;
use crate::{command, filter, projection, record, telemetry, vector};
use std::path::PathBuf;

#[allow(clippy::too_many_arguments)]
pub fn search(
    json: bool,
    store: PathBuf,
    query: Option<String>,
    namespace: Option<String>,
    type_name: Option<String>,
    limit: u16,
    strategy: Option<command::cli::StrategyArg>,
    filters: Vec<String>,
    strict: bool,
    format: command::cli::FormatArg,
    fields: Vec<String>,
    all: bool,
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
    // The pool decides whether a total can be claimed at all. It covers the
    // whole scope whenever a filter or `--all` is in play; otherwise it is just
    // the page, and a count taken from it would only ever say "one page".
    // `--all` promises completeness, and only a path that enumerates can keep
    // that promise. An approximate-neighbour index returns near matches, not
    // every qualifying record, so the refusal is upfront rather than a footnote
    // on an answer that already looks complete.
    // Unset means "pick one for me", and the pick is the one the request can
    // actually be served by: `--all` promises every match, so it gets the path
    // that enumerates. An explicit choice is never overridden — asking for
    // semantics and completeness at once is a contradiction the caller should
    // hear about, not have quietly resolved.
    let strategy = strategy.unwrap_or(if all {
        command::cli::StrategyArg::Fts
    } else {
        command::cli::StrategyArg::Hybrid
    });
    if all && !matches!(strategy, command::cli::StrategyArg::Fts) {
        return Err(Error::Projection(
            "--all needs a path that can enumerate; use --strategy fts, or drop --all".into(),
        ));
    }
    let scope = filter::scope_size(&store, namespace.as_deref(), type_name.as_deref())?;
    let exhaustive = all || !filter.is_empty();
    let pool = if !exhaustive {
        limit
    } else {
        filter::candidate_limit(scope, limit)?
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
    // Everything in scope that matched, counted before the page is cut. This is
    // the number a caller needs to know whether they are looking at the answer
    // or at the first hundred of it.
    let matched = report.hits.len();
    report
        .hits
        .truncate(if all { matched } else { limit as usize });
    vector::finalize(&mut report, matched, pool as usize, exhaustive);
    if report.truncated {
        // stdout stays a clean record stream, so the warning goes beside it:
        // a pipe should never have to strip a sentence out of its data.
        match report.total_matches {
            Some(total) => eprintln!(
                "equill: showing {} of {total} matches; use --all for every match",
                report.returned_count
            ),
            None => eprintln!(
                "equill: showing {} matches; the total is not established here, use --all",
                report.returned_count
            ),
        }
    }
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
