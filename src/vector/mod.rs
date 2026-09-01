pub(crate) mod catchup;
mod config;

pub(crate) use catchup::desired;
pub(crate) use catchup::{drain, worker};

#[cfg(test)]
pub(crate) fn handoff_for_tests(store: &std::path::Path) -> Result<uuid::Uuid, Error> {
    catchup::handoff::claim(store).map(|id| id.expect("a bare store has no live claim"))
}

#[cfg(test)]
pub(crate) fn handoff_path_for_tests(store: &std::path::Path) -> std::path::PathBuf {
    catchup::handoff::path(store)
}

#[cfg(test)]
pub(crate) fn handoff_claim_for_tests(
    store: &std::path::Path,
) -> Result<Option<uuid::Uuid>, Error> {
    catchup::handoff::claim(store)
}

/// Backdate the claim so the startup grace has passed, without sleeping.
#[cfg(test)]
pub(crate) fn age_handoff_for_tests(store: &std::path::Path) {
    catchup::handoff::age_for_tests(store);
}
mod embedder;
mod embedding;
mod fusion;
mod hydrate;
mod model;
mod operator;
mod progress;
mod provider;
mod report;
mod state;

use crate::kernel::error::Error;
use provider::qdrant::{Collection, QdrantTransport, Transport};
use std::path::{Path, PathBuf};

pub use config::{EmbeddingConfig, ModelArtifact, VectorConfig};
pub use drain::{after_commit, after_commit_inline, projection_after_write, resume};
pub use embedder::{Embedder, embed_batch};
pub use embedding::{
    EMBED_MODEL_ID, EmbeddingRuntime, MAX_TOKENS, QUERY_PREFIX, VECTOR_DIMENSIONS,
};
pub use fusion::fuse;
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
// Exposed for the lifecycle fixtures, which drive the two read-path helpers
// directly: they are the whole subject of those tests.
#[cfg(test)]
pub(crate) use operator::{current_only, history_slack, with_semantic_half};
pub use progress::{VectorProgress, VectorProgressSink};
pub use report::freshness;
pub use state::{Freshness, VectorFreshness};
pub use worker::{DrainReport, Projection, run_once, run_worker};

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
    pub fn activate(
        &self,
        physical: &str,
        snapshot: Option<(usize, &str, u64)>,
    ) -> Result<(), Error> {
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
        revision: u64,
    ) -> Result<(), Error> {
        state::stage_ready(
            &self.store,
            &self.config,
            physical,
            Some((records, digest, revision)),
        )?
        .commit()
    }
}

fn activate_collection<T: Transport>(
    store: &Path,
    config: &VectorConfig,
    collection: &Collection<T>,
    physical: &str,
    snapshot: Option<(usize, &str, u64)>,
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
    report::freshness(store, config.as_ref())
}

pub fn state(store: &Path) -> Result<VectorState, Error> {
    let config = config::load(store)?;
    state::read(store, config.as_ref())
}

#[cfg(test)]
pub(crate) mod tests;
