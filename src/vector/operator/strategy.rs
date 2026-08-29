use super::super::model::{VectorSearchRequest, VectorState, vector_error};
use super::super::{EmbeddingRuntime, VectorProjection};
use super::search as retrieval;
use super::search::{RejectedHit, SearchStrategy};
use crate::kernel::error::Error;
use crate::projection::{self, SearchHit, SearchRequest};
use crate::record::StoredRecord;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct StrategySearchReport {
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

/// `fts` is the always-available baseline. `vector` is exact about failure: if
/// the caller asked for semantics it gets an error rather than quietly worse
/// answers. `hybrid` is the forgiving one — it prefers semantics, falls back to
/// text, and says so in the report.
/// An ordinary search answers with what is current. A record a later one
/// replaced, and the tombstone that withdrew it, are both history: returning
/// them here would hand a caller a claim its author has already taken back.
/// `get` and a chain read keep showing them, because that is what auditing is.
/// How many records in this scope a search must be prepared to skip: those a
/// later record replaced, and the tombstones that withdrew them. Read from the
/// canonical ledger rather than the projection, because lifecycle is the
/// ledger's answer to give.
fn history_slack(store_root: &Path, request: &SearchRequest) -> Result<u16, Error> {
    let records = crate::record::read_all(store_root)?;
    let replaced = records
        .iter()
        .filter_map(|record| record.supersedes)
        .collect::<std::collections::HashSet<_>>();
    let history = records
        .iter()
        .filter(|record| {
            request
                .namespace
                .as_ref()
                .is_none_or(|value| &record.namespace == value)
        })
        .filter(|record| {
            request
                .type_name
                .as_ref()
                .is_none_or(|value| &record.type_name == value)
        })
        .filter(|record| {
            replaced.contains(&record.id)
                || record
                    .tags
                    .iter()
                    .any(|tag| tag == crate::record::REVOKED_TAG || tag == "status:revoked")
        })
        .count();
    let needed = usize::from(request.limit).saturating_add(history);
    u16::try_from(needed)
        .ok()
        .filter(|pool| *pool <= crate::projection::MAX_SCAN)
        .ok_or_else(|| {
            // Silently returning a short page here would look like "no more
            // matches" when the truth is "more history than we can scan past".
            vector_error(&format!(
                "this scope holds {history} superseded or withdrawn records, so a page of {} \
                 would need to scan {needed}, past the {} the engine will scan; narrow it by \
                 namespace or type",
                request.limit,
                crate::projection::MAX_SCAN
            ))
        })
}

fn current_only(store_root: &Path, hits: &mut Vec<SearchHit>) -> Result<(), Error> {
    let replaced = projection::superseded(store_root)?;
    hits.retain(|hit| {
        !replaced.contains(&hit.record.id)
            && !hit
                .record
                .tags
                .iter()
                .any(|tag| tag == crate::record::REVOKED_TAG || tag == "status:revoked")
    });
    Ok(())
}

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
            hits.truncate(request.limit as usize);
            Ok(StrategySearchReport {
                ok: true,
                strategy,
                answered_by: "vector",
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

fn semantic(
    store_root: &Path,
    request: &SearchRequest,
) -> Result<(Vec<StoredRecord>, Vec<RejectedHit>), Error> {
    if super::super::state(store_root)? != VectorState::Ready {
        return Err(vector_error("vector projection is not ready"));
    }
    let config = super::super::config::load(store_root)?
        .filter(|config| config.enabled)
        .ok_or_else(|| vector_error("vector projection is not configured"))?;
    let projection = VectorProjection::open(store_root)?
        .ok_or_else(|| vector_error("vector projection is not configured"))?;
    let embedder = EmbeddingRuntime::load(store_root, &config)?;
    // The index ranks history alongside current records, so the page must be
    // asked for with room for however much history could precede it. A guessed
    // multiple is not that: five withdrawn records ahead of one live match
    // would empty a page of one again. The slack is counted, from the ledger —
    // the index itself is not the authority on what a record's lifecycle says.
    let overfetch = history_slack(store_root, request)?;
    let verified = retrieval::retrieve(
        &projection,
        &embedder,
        request.query.as_deref().unwrap_or_default(),
        VectorSearchRequest {
            vector: Vec::new(),
            namespaces: request.namespace.clone().into_iter().collect(),
            type_names: request.type_name.clone().into_iter().collect(),
            limit: overfetch,
        },
    )?;
    Ok((verified.records, verified.rejected))
}

fn text_only(
    store_root: &Path,
    request: &SearchRequest,
    strategy: SearchStrategy,
    state: VectorState,
    fallback: Option<String>,
) -> Result<StrategySearchReport, Error> {
    let mut report = projection::search(store_root, request)?;
    current_only(store_root, &mut report.hits)?;
    Ok(StrategySearchReport {
        ok: report.ok,
        strategy,
        answered_by: "fts",
        vector_state: state,
        fallback,
        rejected: Vec::new(),
        hits: report.hits,
    })
}
