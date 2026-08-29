use super::super::config::VectorConfig;
use super::super::embedding::EmbeddingRuntime;
use super::super::model::{EmbeddingDocument, VectorPoint, VectorPointMetadata, vector_error};
use super::super::{Embedder, VectorProjection, embed_batch};
use super::document::canonical;
use super::rebuild::corpus;
use crate::kernel::error::Error;
use crate::kernel::lock::StoreLock;
use crate::kernel::{identity, store};
use crate::record::StoredRecord;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Instant;
use uuid::Uuid;

const EMBED_BATCH: usize = 32;
const SCAN_BATCH: usize = 256;

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
    fn mark_degraded(&self, physical: &str) -> Result<(), Error>;
    fn mark_ready(&self, physical: &str) -> Result<(), Error>;
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

    fn mark_degraded(&self, physical: &str) -> Result<(), Error> {
        self.mark_degraded(physical)
    }

    fn mark_ready(&self, physical: &str) -> Result<(), Error> {
        self.mark_ready(physical)
    }
}

/// Bring the active collection up to the immutable ledger without creating or
/// switching collections. A long-lived caller can reuse this core operation
/// after a batch append; record writes themselves never load the model.
pub fn sync(store_root: &Path, actor: &str) -> Result<VectorSyncReport, Error> {
    let store_config = store::load(store_root)?;
    identity::require_root(&store_config, actor)?;
    let vector_config = super::super::config::load(store_root)?
        .filter(|config| config.enabled)
        .ok_or_else(|| vector_error("vector projection is not configured"))?;
    let projection = VectorProjection::open(store_root)?
        .ok_or_else(|| vector_error("vector projection is not configured"))?;
    execute(store_root, &vector_config, &projection, || {
        EmbeddingRuntime::load(store_root, &vector_config)
    })
}

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
    let started = Instant::now();
    let physical = index.active_collection()?;
    let (records, digest) = {
        let _lock = StoreLock::exclusive(store_root)?;
        corpus(store_root)?
    };
    let documents = pending(config, index, &physical, &records)?;
    let embeddings = documents.len();
    let mut upsert_batches = 0;

    if !documents.is_empty() {
        let _lock = StoreLock::exclusive(store_root)?;
        require_unchanged(store_root, &digest)?;
        index.mark_degraded(&physical)?;
        drop(_lock);
        let embedder = load_embedder()?;
        verify_descriptor(config, &embedder)?;
        for chunk in documents.chunks(EMBED_BATCH) {
            let points = embed_batch(&embedder, chunk)?;
            index.upsert(&physical, &points)?;
            upsert_batches += 1;
        }
        if !pending(config, index, &physical, &records)?.is_empty() {
            return Err(vector_error("incremental sync verification failed"));
        }
    }

    let _lock = StoreLock::exclusive(store_root)?;
    require_unchanged(store_root, &digest)?;
    index.ensure_active(&physical)?;
    if super::super::state::read(store_root, Some(config))? != super::super::VectorState::Ready {
        index.mark_ready(&physical)?;
    }
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

fn require_unchanged(store_root: &Path, expected: &str) -> Result<(), Error> {
    let (_, recheck) = corpus(store_root)?;
    if recheck != expected {
        return Err(vector_error(
            "the ledger changed during vector sync; rerun vector sync",
        ));
    }
    Ok(())
}

fn pending<I: SyncIndex>(
    config: &VectorConfig,
    index: &I,
    physical: &str,
    records: &[(StoredRecord, String)],
) -> Result<Vec<EmbeddingDocument>, Error> {
    let mut pending = Vec::new();
    for chunk in records.chunks(SCAN_BATCH) {
        let ids = chunk
            .iter()
            .map(|(record, _)| record.id)
            .collect::<Vec<_>>();
        let requested = ids.iter().copied().collect::<HashSet<_>>();
        let mut current = HashMap::new();
        for item in index.metadata(physical, &ids)? {
            if !requested.contains(&item.record_id)
                || current.insert(item.record_id, item).is_some()
            {
                return Err(vector_error("retrieval returned unexpected point metadata"));
            }
        }
        for (record, record_sha256) in chunk {
            let document = canonical(record, record_sha256)?;
            let compatible = current.get(&record.id).is_some_and(|item| {
                item.record_sha256 == document.record_sha256
                    && item.input_sha256 == document.input_sha256
                    && item.model_sha256 == config.embedding.model.sha256
            });
            if !compatible {
                pending.push(document);
            }
        }
    }
    Ok(pending)
}

fn verify_descriptor(config: &VectorConfig, embedder: &impl Embedder) -> Result<(), Error> {
    let descriptor = embedder.descriptor();
    if descriptor.model_id != config.embedding.model_id
        || descriptor.model_sha256 != config.embedding.model.sha256
        || descriptor.tokenizer_sha256 != config.embedding.tokenizer.sha256
        || descriptor.dimensions != config.dimensions
        || descriptor.distance != config.distance
        || descriptor.input_schema != config.embedding.input_schema
    {
        return Err(vector_error("embedder does not match vector configuration"));
    }
    Ok(())
}
