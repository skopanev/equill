//! Asking the index alone, and the seam that lets a test answer for it.
//!
//! Split out so the merge above can be exercised without a running Qdrant: a
//! cross-surface test needs a semantic list it chose, not one a live provider
//! happened to return, or it would be asserting about the provider instead of
//! about the contract.
use super::super::model::{VectorSearchRequest, VectorState, vector_error};
use super::super::{EmbeddingRuntime, VectorProjection};
use super::search as retrieval;
use super::search::RejectedHit;
use super::strategy::history_slack;
use crate::kernel::error::Error;
use crate::projection::SearchRequest;
use crate::record::StoredRecord;
use std::path::Path;

#[cfg(test)]
type Half = fn(&Path, &SearchRequest) -> Result<(Vec<StoredRecord>, Vec<RejectedHit>), Error>;

#[cfg(test)]
thread_local! {
    static HALF: std::cell::Cell<Option<Half>> = const { std::cell::Cell::new(None) };
}

/// Clears the substitution however the body leaves.
///
/// A test that asserts and fails unwinds, and a plain "set, run, unset" would
/// leave the substitute installed for whatever the harness runs next on this
/// thread. One failure would then be followed by a second that has nothing to
/// do with its own subject — the worst kind, because it points at the wrong
/// code.
#[cfg(test)]
struct Restore;

#[cfg(test)]
impl Drop for Restore {
    fn drop(&mut self) {
        HALF.with(|slot| slot.set(None));
    }
}

/// Answer the semantic half from `half` for the duration of `body`.
///
/// Test-only and thread-local, like the worker's spawn seam: the release build
/// does not contain it, and parallel tests do not see each other's substitute.
#[cfg(test)]
pub(crate) fn with_semantic_half<T>(half: Half, body: impl FnOnce() -> T) -> T {
    HALF.with(|slot| slot.set(Some(half)));
    let _restore = Restore;
    body()
}

pub(super) fn semantic(
    store_root: &Path,
    request: &SearchRequest,
) -> Result<(Vec<StoredRecord>, Vec<RejectedHit>), Error> {
    #[cfg(test)]
    if let Some(injected) = HALF.with(|slot| slot.get()) {
        return injected(store_root, request);
    }
    live(store_root, request)
}

fn live(
    store_root: &Path,
    request: &SearchRequest,
) -> Result<(Vec<StoredRecord>, Vec<RejectedHit>), Error> {
    // Health, not freshness: a lagging index still answers from the points it
    // has. Refusing here would mean one append silences semantic search until
    // the next sync, which is exactly what a continuously written store cannot
    // afford. The report says how far behind the answer is.
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
