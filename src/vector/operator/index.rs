//! The index surface a catch-up needs, and how the real projection provides it.
use super::super::VectorProjection;
use super::super::model::EmbeddingDocument;
use super::super::model::{VectorPoint, VectorPointMetadata};
use crate::kernel::error::Error;
use uuid::Uuid;

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
    ) -> Result<(), Error> {
        self.mark_indexed(physical, records, digest, revision)
    }
}
