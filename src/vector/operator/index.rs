//! The index surface a catch-up needs, and how the real projection provides it.
use super::super::VectorProjection;
use super::super::model::EmbeddingDocument;
use super::super::model::{VectorPoint, VectorPointMetadata};
use crate::kernel::error::Error;
use uuid::Uuid;
#[cfg(test)]
use {super::sync::VectorSyncReport, std::path::Path};

pub(crate) trait SyncIndex {
    fn active_collection(&self) -> Result<String, Error>;
    fn metadata(
        &self,
        physical: &str,
        record_ids: &[Uuid],
    ) -> Result<Vec<VectorPointMetadata>, Error>;
    fn upsert(&self, physical: &str, points: &[VectorPoint]) -> Result<(), Error>;
    /// Update a point's bookkeeping without touching its vector.
    ///
    /// Needed because an envelope can change while the meaning does not — a
    /// compaction cutting a `supersedes` link is exactly that — and re-running
    /// the model to write a new hash into the payload would cost the whole
    /// corpus for a field the model never reads.
    fn relabel(&self, physical: &str, documents: &[EmbeddingDocument]) -> Result<(), Error>;
    fn delete(&self, physical: &str, record_ids: &[Uuid]) -> Result<(), Error>;
    fn ensure_active(&self, physical: &str) -> Result<(), Error>;
    fn mark_indexed(
        &self,
        physical: &str,
        records: usize,
        digest: &str,
        revision: u64,
        embed_types_sha256: Option<&str>,
    ) -> Result<(), Error>;
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

    fn delete(&self, physical: &str, record_ids: &[Uuid]) -> Result<(), Error> {
        self.delete(physical, record_ids)
    }

    fn relabel(&self, physical: &str, documents: &[EmbeddingDocument]) -> Result<(), Error> {
        self.relabel(physical, documents)
    }

    fn ensure_active(&self, physical: &str) -> Result<(), Error> {
        self.ensure_active(physical)
    }

    fn mark_indexed(
        &self,
        physical: &str,
        records: usize,
        digest: &str,
        revision: u64,
        embed_types_sha256: Option<&str>,
    ) -> Result<(), Error> {
        self.mark_indexed(physical, records, digest, revision, embed_types_sha256)
    }
}

/// A stand-in for the catch-up, installed for one call.
///
/// Beside the index trait rather than in the pass itself: both are here so a
/// test can put something of its own where the real thing goes, and the pass
/// file had no room left for machinery only tests use.
#[cfg(test)]
type Standin = std::sync::Arc<dyn Fn(&Path, &str) -> Result<VectorSyncReport, Error> + Send + Sync>;

#[cfg(test)]
thread_local! {
    static STANDIN: std::cell::RefCell<Option<Standin>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn substituted(
    store_root: &Path,
    alias: &str,
) -> Option<Result<VectorSyncReport, Error>> {
    STANDIN
        .with(|slot| slot.borrow().clone())
        .map(|standin| standin(store_root, alias))
}

/// Installed for one call and removed after, even if the body panics.
#[cfg(test)]
pub(crate) fn with_standin<T>(standin: Standin, body: impl FnOnce() -> T) -> T {
    struct Restore(Option<Standin>);
    impl Drop for Restore {
        fn drop(&mut self) {
            STANDIN.with(|slot| *slot.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(STANDIN.with(|slot| slot.borrow_mut().take()));
    STANDIN.with(|slot| *slot.borrow_mut() = Some(standin));
    body()
}
