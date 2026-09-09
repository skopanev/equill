//! The operator half of the vector projection: what Equill does with an index,
//! kept apart from the provider that talks to Qdrant.
mod configure;
mod delta;
mod document;
mod index;
mod rebuild;
mod search;
mod semantic;
mod strategy;
mod sync;

pub use configure::{VectorConfigReport, configure, disable};
#[cfg(test)]
pub(crate) use configure::{announce_outstanding_work, store_descriptor, undo};
pub use document::canonical;
#[cfg(test)]
pub(crate) use index::SyncIndex;
#[cfg(test)]
pub(crate) use rebuild::capture;
pub use rebuild::{VectorRebuildReport, rebuild, rebuild_with_progress};
pub use search::{
    QueryEmbedder, RejectedHit, SearchStrategy, VectorIndex, VerifiedHits, retrieve, verify,
};
pub(crate) use strategy::search_with_policy;
pub use strategy::{StrategySearchReport, finalize, search, search_with};
#[cfg(test)]
pub(crate) use strategy::{current_only, history_slack};
pub(crate) use sync::catch_up;
#[cfg(test)]
pub(crate) use sync::with_standin;
pub use sync::{VectorSyncReport, sync, sync_with_progress};
#[cfg(test)]
pub(crate) use sync::{execute, execute_with_progress};

// Exposed for the cross-surface fixture, which supplies the semantic half so
// the merge can be checked without a live index.
#[cfg(test)]
pub(crate) use semantic::with_semantic_half;
