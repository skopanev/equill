use super::super::VectorProjection;
use super::super::model::{VectorSearchHit, VectorSearchRequest, vector_error};
use super::document;
use crate::kernel::digest::sha256_hex;
use crate::kernel::error::Error;
use crate::record::StoredRecord;
use serde::Serialize;

/// The two provider-facing capabilities the operator needs. They are traits so
/// the operator can be exercised without a live Qdrant and without weights.
pub trait VectorIndex {
    fn search(&self, request: &VectorSearchRequest) -> Result<Vec<VectorSearchHit>, Error>;
}

pub trait QueryEmbedder {
    fn embed_query(&self, query: &str) -> Result<Vec<f32>, Error>;
}

impl VectorIndex for VectorProjection {
    fn search(&self, request: &VectorSearchRequest) -> Result<Vec<VectorSearchHit>, Error> {
        VectorProjection::search(self, request)
    }
}

impl QueryEmbedder for super::super::EmbeddingRuntime {
    fn embed_query(&self, query: &str) -> Result<Vec<f32>, Error> {
        super::super::EmbeddingRuntime::embed_query(self, query)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchStrategy {
    Fts,
    Vector,
    Hybrid,
}

/// Why a candidate the index returned was not allowed to reach the caller. The
/// reason is a coordinate and a verdict, never payload text.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RejectedHit {
    pub record_id: uuid::Uuid,
    pub reason: &'static str,
}

#[derive(Debug, Serialize)]
pub struct VerifiedHits {
    pub records: Vec<StoredRecord>,
    pub rejected: Vec<RejectedHit>,
}

/// Hydration in the provider already refuses a candidate that names no record
/// in this store or whose record hash moved. What it cannot know is whether the
/// vector still describes the record: an edit that keeps the ledger honest can
/// still leave an embedding behind. So the canonical input is re-derived and
/// compared, and anything stale is dropped rather than returned.
pub fn verify(hits: Vec<VectorSearchHit>, limit: usize) -> Result<VerifiedHits, Error> {
    let mut records = Vec::new();
    let mut rejected = Vec::new();
    for hit in hits {
        let digest = sha256_hex(&serde_json::to_vec(&hit.record)?);
        match document::canonical(&hit.record, &digest) {
            Ok(document) if document.input_sha256 == hit.input_sha256 => {
                records.push(hit.record);
                if records.len() == limit {
                    break;
                }
            }
            Ok(_) => rejected.push(reject(&hit, "embedding input is stale")),
            Err(_) => rejected.push(reject(&hit, "record has no canonical input")),
        }
    }
    Ok(VerifiedHits { records, rejected })
}

/// Retrieval for one query: embed, ask the index, then re-derive.
pub fn retrieve(
    index: &impl VectorIndex,
    embedder: &impl QueryEmbedder,
    query: &str,
    request: VectorSearchRequest,
) -> Result<VerifiedHits, Error> {
    if query.trim().is_empty() {
        return Err(vector_error("search requires a query"));
    }
    let limit = request.limit as usize;
    let vector = embedder.embed_query(query)?;
    verify(
        index.search(&VectorSearchRequest { vector, ..request })?,
        limit,
    )
}

fn reject(hit: &VectorSearchHit, reason: &'static str) -> RejectedHit {
    RejectedHit {
        record_id: hit.record.id,
        reason,
    }
}
