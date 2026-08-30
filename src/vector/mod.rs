mod config;
mod desired;
mod drain;
mod embedder;
mod embedding;
mod hydrate;
mod model;
mod operator;
mod progress;
mod provider;
mod state;

use crate::kernel::error::Error;
use provider::qdrant::{Collection, QdrantTransport, Transport};
use std::path::{Path, PathBuf};

pub use config::{EmbeddingConfig, ModelArtifact, VectorConfig};
pub use drain::{DrainReport, after_commit};
pub use embedder::{Embedder, embed_batch};
pub use embedding::{
    EMBED_MODEL_ID, EmbeddingRuntime, MAX_TOKENS, QUERY_PREFIX, VECTOR_DIMENSIONS,
};
pub use model::{
    CollectionReport, DistanceMetric, EmbeddingDescriptor, EmbeddingDocument, INPUT_SCHEMA,
    VectorPoint, VectorSearchHit, VectorSearchRequest, VectorState,
};
pub(crate) use operator::corpus;
pub use operator::finalize;
pub use operator::{
    QueryEmbedder, RejectedHit, SearchStrategy, StrategySearchReport, VectorConfigReport,
    VectorIndex, VectorRebuildReport, VectorSyncReport, VerifiedHits, canonical, configure,
    disable, rebuild, rebuild_with_progress, retrieve, search, sync, sync_with_progress, verify,
};
pub use progress::{VectorProgress, VectorProgressSink};
pub use state::{Freshness, VectorFreshness, freshness};

pub struct VectorProjection {
    store: PathBuf,
    config: VectorConfig,
    collection: Collection<QdrantTransport>,
}

impl VectorProjection {
    pub fn open(store: &Path) -> Result<Option<Self>, Error> {
        let Some(config) = config::load(store)?.filter(|config| config.enabled) else {
            return Ok(None);
        };
        let transport = QdrantTransport::new(&config)?;
        Ok(Some(Self {
            store: store.to_owned(),
            config: config.clone(),
            collection: Collection::new(config, transport),
        }))
    }

    pub fn prepare_collection(&self, physical: &str) -> Result<CollectionReport, Error> {
        self.collection.prepare(physical)
    }

    pub fn upsert(&self, physical: &str, points: &[VectorPoint]) -> Result<(), Error> {
        self.collection.upsert(physical, points)
    }

    pub fn search(&self, request: &VectorSearchRequest) -> Result<Vec<VectorSearchHit>, Error> {
        let candidates = self.collection.search(request)?;
        hydrate::from_ledger(&self.store, request, candidates)
    }

    /// The snapshot the index was built from travels with activation, so the
    /// checkpoint and the collection it describes are committed together.
    pub fn activate(&self, physical: &str, snapshot: Option<(usize, &str)>) -> Result<(), Error> {
        activate_collection(
            &self.store,
            &self.config,
            &self.collection,
            physical,
            snapshot,
        )
    }

    pub(crate) fn active_collection(&self) -> Result<String, Error> {
        self.collection.active()
    }

    pub(crate) fn metadata(
        &self,
        physical: &str,
        record_ids: &[uuid::Uuid],
    ) -> Result<Vec<model::VectorPointMetadata>, Error> {
        self.collection.metadata(physical, record_ids)
    }

    pub(crate) fn ensure_active(&self, physical: &str) -> Result<(), Error> {
        self.collection.require_active(physical)
    }

    pub(crate) fn mark_indexed(
        &self,
        physical: &str,
        records: usize,
        digest: &str,
    ) -> Result<(), Error> {
        state::stage_ready(&self.store, &self.config, physical, Some((records, digest)))?.commit()
    }
}

fn activate_collection<T: Transport>(
    store: &Path,
    config: &VectorConfig,
    collection: &Collection<T>,
    physical: &str,
    snapshot: Option<(usize, &str)>,
) -> Result<(), Error> {
    let marker = state::stage_ready(store, config, physical, snapshot)?;
    let change = collection.activate(physical)?;
    if let Err(error) = marker.commit() {
        if collection.restore(&change).is_err() {
            return Err(model::vector_error(
                "ready marker failed and alias rollback failed",
            ));
        }
        return Err(error);
    }
    Ok(())
}

/// Freshness for a caller that already knows the store: the config is loaded
/// once here rather than re-read by every reporting surface.
pub fn freshness_of(store: &Path) -> Result<Freshness, Error> {
    let config = config::load(store)?;
    state::freshness(store, config.as_ref())
}

pub fn state(store: &Path) -> Result<VectorState, Error> {
    let config = config::load(store)?;
    state::read(store, config.as_ref())
}

#[cfg(test)]
mod tests;
