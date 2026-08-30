//! The operator half of the vector projection: what Equill does with an index,
//! kept apart from the provider that talks to Qdrant.
mod configure;
mod delta;
mod document;
mod index;
mod rebuild;
mod search;
mod strategy;
mod sync;

pub use configure::{VectorConfigReport, configure, disable};
pub use document::canonical;
#[cfg(test)]
pub(crate) use index::SyncIndex;
pub(crate) use rebuild::corpus;
pub use rebuild::{VectorRebuildReport, rebuild, rebuild_with_progress};
pub use search::{
    QueryEmbedder, RejectedHit, SearchStrategy, VectorIndex, VerifiedHits, retrieve, verify,
};
pub use strategy::{StrategySearchReport, finalize, search};
pub(crate) use sync::catch_up;
pub use sync::{VectorSyncReport, sync, sync_with_progress};
#[cfg(test)]
pub(crate) use sync::{execute, execute_with_progress};
