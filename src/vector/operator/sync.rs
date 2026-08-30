use super::super::config::VectorConfig;
use super::super::embedding::EmbeddingRuntime;
use super::super::model::{VectorPoint, VectorPointMetadata, vector_error};
use super::super::progress::{VectorProgress, VectorProgressSink, emit};
use super::super::{Embedder, VectorProjection, embed_batch};
use super::delta::{pending, verify_descriptor};
use super::rebuild::corpus;
use crate::kernel::error::Error;
use crate::kernel::lock::StoreLock;
use crate::kernel::{identity, store};
use serde::Serialize;
use std::path::Path;
use std::time::Instant;
use uuid::Uuid;

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

pub(crate) trait SyncIndex {
    fn active_collection(&self) -> Result<String, Error>;
    fn metadata(
        &self,
        physical: &str,
        record_ids: &[Uuid],
    ) -> Result<Vec<VectorPointMetadata>, Error>;
    fn upsert(&self, physical: &str, points: &[VectorPoint]) -> Result<(), Error>;
    fn ensure_active(&self, physical: &str) -> Result<(), Error>;
    fn mark_indexed(&self, physical: &str, records: usize, digest: &str) -> Result<(), Error>;
}

impl SyncIndex for VectorProjection {
    fn active_collection(&self) -> Result<String, Error> {
        self.active_collection()
    }

    fn metadata(
        &self,
        physical: &str,
        record_ids: &[Uuid],
    ) -> Result<Vec<VectorPointMetadata>, Error> {
        self.metadata(physical, record_ids)
    }

    fn upsert(&self, physical: &str, points: &[VectorPoint]) -> Result<(), Error> {
        self.upsert(physical, points)
    }

    fn ensure_active(&self, physical: &str) -> Result<(), Error> {
        self.ensure_active(physical)
    }

    fn mark_indexed(&self, physical: &str, records: usize, digest: &str) -> Result<(), Error> {
        self.mark_indexed(physical, records, digest)
    }
}

/// Bring the active collection up to the immutable ledger without creating or
/// switching collections. A long-lived caller can reuse this core operation
/// after a batch append; record writes themselves never load the model.
pub fn sync(store_root: &Path, actor: &str) -> Result<VectorSyncReport, Error> {
    sync_with_progress(store_root, actor, None)
}

pub fn sync_with_progress(
    store_root: &Path,
    actor: &str,
    mut progress: Option<&mut dyn VectorProgressSink>,
) -> Result<VectorSyncReport, Error> {
    let store_config = store::load(store_root)?;
    identity::require_root(&store_config, actor)?;
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
    let (records, digest) = {
        let _lock = StoreLock::exclusive(store_root)?;
        corpus(store_root)?
    };
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

    let _lock = StoreLock::exclusive(store_root)?;
    index.ensure_active(&physical)?;
    // The checkpoint records what this pass actually covered, so the next one
    // knows its tail and a reader can tell how far behind the index is.
    index.mark_indexed(&physical, records.len(), &digest)?;
    drop(_lock);
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
