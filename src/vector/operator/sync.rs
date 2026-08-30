use super::super::config::VectorConfig;
use super::super::embedding::EmbeddingRuntime;
use super::super::model::vector_error;
use super::super::progress::{VectorProgress, VectorProgressSink, emit};
use super::super::{Embedder, VectorProjection, embed_batch};
use super::delta::{pending, verify_descriptor};
use super::index::SyncIndex;
use super::rebuild::corpus;
use crate::kernel::error::Error;
use crate::kernel::governance::RootGuard;
use crate::kernel::lock::StoreLock;

use serde::Serialize;
use std::path::Path;
use std::time::Instant;

const EMBED_BATCH: usize = 32;

#[derive(Debug, Serialize)]
pub struct VectorSyncReport {
    pub ok: bool,
    pub projection: &'static str,
    pub collection: String,
    pub records: usize,
    pub embeddings: usize,
    pub points_upserted: usize,
    pub upsert_batches: usize,
    pub corpus_sha256: String,
    pub duration_ms: u64,
}

/// Bring the active collection up to the immutable ledger without creating or
/// switching collections. A long-lived caller can reuse this core operation
/// after a batch append; record writes themselves never load the model.
pub(crate) fn catch_up(store_root: &Path) -> Result<VectorSyncReport, Error> {
    catch_up_with_progress(store_root, None)
}

pub fn sync(store_root: &Path, actor: &str) -> Result<VectorSyncReport, Error> {
    sync_with_progress(store_root, actor, None)
}

pub fn sync_with_progress(
    store_root: &Path,
    actor: &str,
    progress: Option<&mut dyn VectorProgressSink>,
) -> Result<VectorSyncReport, Error> {
    // Running a sync on demand is governance: it is the owner who decides when
    // the store spends minutes on a model. Reaching the same work as a
    // consequence of a write one was already allowed to make is not, which is
    // why the internal entry below exists and does not ask again.
    let (_guard, _config) = RootGuard::acquire(store_root, actor)?;
    // An operator asking for the work is not something a remembered failure gets
    // to refuse: the explicit path always runs, and clears the way for the
    // automatic one at the same time.
    crate::vector::catchup::cooldown::clear(store_root);
    catch_up_with_progress(store_root, progress)
}

/// The catch-up itself, without an authorization question. Reachable only after
/// a canonical append has already been validated and allowed, so asking the
/// writer to also be the store owner would deny every scoped writer the index
/// their own writes just changed — while giving them no way to fix it.
pub(crate) fn catch_up_with_progress(
    store_root: &Path,
    mut progress: Option<&mut dyn VectorProgressSink>,
) -> Result<VectorSyncReport, Error> {
    let vector_config = super::super::config::load(store_root)?
        .filter(|config| config.enabled)
        .ok_or_else(|| vector_error("vector projection is not configured"))?;
    emit(
        &mut progress,
        VectorProgress::Connecting {
            collection: vector_config.collection_alias.clone(),
        },
    );
    let projection = VectorProjection::open(store_root)?
        .ok_or_else(|| vector_error("vector projection is not configured"))?;
    execute_with_progress(
        store_root,
        &vector_config,
        &projection,
        || EmbeddingRuntime::load(store_root, &vector_config),
        progress,
    )
}

#[cfg(test)]
pub(crate) fn execute<I, E, F>(
    store_root: &Path,
    config: &VectorConfig,
    index: &I,
    load_embedder: F,
) -> Result<VectorSyncReport, Error>
where
    I: SyncIndex,
    E: Embedder,
    F: FnOnce() -> Result<E, Error>,
{
    execute_with_progress(store_root, config, index, load_embedder, None)
}

pub(crate) fn execute_with_progress<I, E, F>(
    store_root: &Path,
    config: &VectorConfig,
    index: &I,
    load_embedder: F,
    mut progress: Option<&mut dyn VectorProgressSink>,
) -> Result<VectorSyncReport, Error>
where
    I: SyncIndex,
    E: Embedder,
    F: FnOnce() -> Result<E, Error>,
{
    let started = Instant::now();
    let physical = index.active_collection()?;
    // Read the target BEFORE the corpus: a write that lands while this pass runs
    // must leave the checkpoint behind the target rather than be swallowed by it.
    let revision = crate::vector::desired::read(store_root)?.map_or(0, |target| target.revision);
    // Deliberately NOT under the writer lock: holding it across a full ledger
    // hash made every concurrent write wait for the scan (measured p95 165ms and
    // 873ms). The ledger is append-only and the reader stops at the last
    // completed line, so an unlocked snapshot is a consistent prefix.
    let (records, digest) = corpus(store_root)?;
    let documents = pending(config, index, &physical, &records)?;
    let embeddings = documents.len();
    let mut upsert_batches = 0;
    emit(
        &mut progress,
        VectorProgress::Scanned {
            collection: physical.clone(),
            records: records.len(),
            pending: embeddings,
            corpus_sha256: digest.clone(),
        },
    );

    if !documents.is_empty() {
        // Only the captured snapshot is processed. Whatever is appended while
        // the model runs is the next call's tail, not a reason to fail this one
        // — a store that is written to continuously would otherwise never
        // finish a sync at all.
        //
        // The previous checkpoint stays Ready for the whole pass. Demoting it
        // here would take semantic search offline for as long as the model runs
        // and, worse, leave it offline if the pass failed — losing a working
        // index to protect it from being slightly behind.
        emit(&mut progress, VectorProgress::LoadingModel);
        let embedder = load_embedder()?;
        verify_descriptor(config, &embedder)?;
        let mut completed = 0;
        for chunk in documents.chunks(EMBED_BATCH) {
            let points = embed_batch(&embedder, chunk)?;
            completed += points.len();
            emit(
                &mut progress,
                VectorProgress::Embedded {
                    completed,
                    total: embeddings,
                },
            );
            index.upsert(&physical, &points)?;
            upsert_batches += 1;
            emit(
                &mut progress,
                VectorProgress::Upserted {
                    completed,
                    total: embeddings,
                },
            );
        }
        if !pending(config, index, &physical, &records)?.is_empty() {
            return Err(vector_error("incremental sync verification failed"));
        }
    }

    // Before the lock: this asks the provider, and a network call under the
    // writer lock makes every concurrent write wait on it.
    index.ensure_active(&physical)?;
    {
        // The checkpoint records what this pass covered, so the next one knows
        // its tail. Only the marker write needs the lock.
        let _lock = StoreLock::exclusive(store_root)?;
        index.mark_indexed(&physical, records.len(), &digest, revision)?;
    }
    emit(
        &mut progress,
        VectorProgress::Ready {
            collection: physical.clone(),
            corpus_sha256: digest.clone(),
        },
    );
    Ok(VectorSyncReport {
        ok: true,
        projection: "vector-qdrant",
        collection: physical,
        records: records.len(),
        embeddings,
        points_upserted: embeddings,
        upsert_batches,
        corpus_sha256: digest,
        duration_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
    })
}
