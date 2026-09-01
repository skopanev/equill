use super::super::model::VectorState;
use super::search::{RejectedHit, SearchStrategy};
use super::semantic::semantic;
use crate::kernel::error::Error;
use crate::projection::{self, SearchHit, SearchRequest};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct StrategySearchReport {
    /// How many records matched in the scope the caller asked about, against
    /// how many this page carries. Without both, a full page and a truncated
    /// one are indistinguishable — which is how a page gets mistaken for the
    /// whole answer.
    ///
    /// Absent for a semantic answer on purpose: an approximate-neighbour index
    /// returns the closest points it found, not every record that would have
    /// qualified. Reporting its page size as a total would be a number we
    /// cannot stand behind, and a wrong total is worse than none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_matches: Option<usize>,
    pub returned_count: usize,
    pub truncated: bool,
    /// Health and freshness, kept apart on purpose: an index can answer well
    /// and still be behind, and a caller deserves to know which it got.
    pub vector_freshness: crate::vector::VectorFreshness,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector_indexed_records: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector_pending_records: Option<usize>,
    pub ok: bool,
    pub strategy: SearchStrategy,
    /// What actually answered, which is not always what was asked for.
    pub answered_by: &'static str,
    pub vector_state: VectorState,
    /// Present when the vector half could not answer and text search stood in.
    /// It is part of the report, not a log line, so a caller can see in the
    /// receipt that it received a degraded answer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rejected: Vec<RejectedHit>,
    pub hits: Vec<SearchHit>,
}

/// How many records in this scope a semantic page must be prepared to skip:
/// those a later record replaced, and the tombstones that withdrew them. Read
/// from the projection's indexed lifecycle, which is a projection of the same
/// ledger facts and answers without walking it.
pub(crate) fn history_slack(store_root: &Path, request: &SearchRequest) -> Result<u16, Error> {
    let history = crate::projection::history_in_scope(
        store_root,
        &crate::projection::LifecycleScope {
            namespace: request.namespace.clone(),
            type_name: request.type_name.clone(),
        },
    )?
    .history;
    let needed = usize::from(request.limit).saturating_add(history);
    // An index request is a u16, so that is the bound. This used to refuse the
    // query outright once the pool passed the projection's scan cap, telling
    // the caller to narrow by namespace or type — a refusal to serve an answer
    // that existed, over a number the caller never chose.
    Ok(u16::try_from(needed).unwrap_or(u16::MAX))
}

/// An ordinary search answers with what is current. A record a later one
/// replaced, and the tombstone that withdrew it, are both history: returning
/// them here would hand a caller a claim its author has already taken back.
/// `get` and a chain read keep showing them, because that is what auditing is.
///
/// A record the projection has not indexed yet is left alone rather than
/// dropped: silence about a record is not evidence against it, and a read that
/// discarded everything the projection had not caught up on would lose live
/// answers exactly when the store is busiest.
pub(crate) fn current_only(store_root: &Path, hits: &mut Vec<SearchHit>) -> Result<(), Error> {
    let ids = hits.iter().map(|hit| hit.record.id).collect::<Vec<_>>();
    let history = crate::projection::historic(store_root, &ids)?.history;
    hits.retain(|hit| !history.contains(&hit.record.id));
    Ok(())
}

/// Settles what a result set may claim about itself, in one place, so a caller
/// on either surface gets the same answer to "is this all of it".
///
/// A total is reported only when the pool actually covered the scope and the
/// answer came from a path that enumerates. A limited pool that filled up
/// proves the opposite: there is more, and how much is unknown.
pub fn finalize(report: &mut StrategySearchReport, matched: usize, pool: usize, exhaustive: bool) {
    report.returned_count = report.hits.len();
    // Only a text answer can prove a total. Stated as what qualifies rather
    // than what does not: naming the exceptions meant that adding `hybrid`
    // silently made it provable, and a merged page would have printed the text
    // half's count as the whole answer's.
    let provable = exhaustive && report.answered_by == "fts";
    report.total_matches = provable.then_some(matched);
    report.truncated = report.returned_count < matched || (!provable && matched >= pool);
}

/// `fts` is the always-available baseline. `vector` is exact about failure: if
/// the caller asked for semantics it gets an error rather than quietly worse
/// answers. `hybrid` is the forgiving one — it prefers semantics, falls back to
/// text, and says so in the report.
pub fn search(
    store_root: &Path,
    request: &SearchRequest,
    strategy: SearchStrategy,
) -> Result<StrategySearchReport, Error> {
    let state = super::super::state(store_root)?;
    if strategy == SearchStrategy::Fts {
        return text_only(store_root, request, strategy, state, None);
    }
    match semantic(store_root, request) {
        Ok((records, rejected)) => {
            let mut hits = records
                .into_iter()
                .map(|record| SearchHit { record })
                .collect::<Vec<_>>();
            current_only(store_root, &mut hits)?;
            // Hybrid asks both and merges what each found; `vector` asks the
            // index alone. That difference is the whole reason the two have
            // separate names, and until now they behaved identically whenever
            // semantics answered at all.
            let answered_by = if strategy == SearchStrategy::Hybrid {
                let text = projection::search(store_root, request)?.hits;
                hits = crate::vector::fuse(hits, text);
                "hybrid"
            } else {
                "vector"
            };
            hits.truncate(request.limit as usize);
            let reading = crate::vector::freshness_of(store_root)?;
            Ok(StrategySearchReport {
                total_matches: None,
                returned_count: hits.len(),
                truncated: false,
                vector_freshness: reading.freshness,
                vector_indexed_records: reading.indexed_records,
                vector_pending_records: reading.pending_records,
                ok: true,
                strategy,
                answered_by,
                vector_state: state,
                fallback: None,
                rejected,
                hits,
            })
        }
        Err(error) if strategy == SearchStrategy::Hybrid => text_only(
            store_root,
            request,
            strategy,
            state,
            Some(error.to_string()),
        ),
        Err(error) => Err(error),
    }
}

fn text_only(
    store_root: &Path,
    request: &SearchRequest,
    strategy: SearchStrategy,
    state: VectorState,
    fallback: Option<String>,
) -> Result<StrategySearchReport, Error> {
    // Full text and the filter-only scan exclude history in the query itself,
    // so there is nothing left here to filter out.
    let report = projection::search(store_root, request)?;
    let reading = crate::vector::freshness_of(store_root)?;
    Ok(StrategySearchReport {
        total_matches: Some(report.hits.len()),
        returned_count: report.hits.len(),
        truncated: false,
        vector_freshness: reading.freshness,
        vector_indexed_records: reading.indexed_records,
        vector_pending_records: reading.pending_records,
        ok: report.ok,
        strategy,
        answered_by: "fts",
        vector_state: state,
        fallback,
        rejected: Vec::new(),
        hits: report.hits,
    })
}
