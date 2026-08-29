use crate::kernel::error::Error;
use crate::record::StoredRecord;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const INPUT_SCHEMA: &str = "equill.record.embedding.v1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DistanceMetric {
    Cosine,
    Dot,
    Euclid,
    Manhattan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VectorState {
    Disabled,
    Ready,
    Degraded,
    Missing,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EmbeddingDescriptor {
    pub model_id: String,
    pub model_sha256: String,
    pub tokenizer_sha256: String,
    pub dimensions: u64,
    pub distance: DistanceMetric,
    pub input_schema: String,
}

pub struct EmbeddingDocument {
    pub record_id: Uuid,
    pub namespace: String,
    pub type_name: String,
    pub record_sha256: String,
    pub input_sha256: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct VectorPoint {
    pub record_id: Uuid,
    pub namespace: String,
    pub type_name: String,
    pub record_sha256: String,
    pub input_sha256: String,
    pub vector: Vec<f32>,
}

#[derive(Clone, Debug)]
pub struct VectorSearchRequest {
    pub vector: Vec<f32>,
    pub namespaces: Vec<String>,
    pub type_names: Vec<String>,
    pub limit: u16,
}

#[derive(Clone, Debug, Serialize)]
pub struct VectorSearchHit {
    pub record: StoredRecord,
    pub score: f32,
    pub input_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CollectionReport {
    pub collection: String,
    pub created: bool,
}

pub(crate) fn validate_point(point: &VectorPoint, dimensions: u64) -> Result<(), Error> {
    if point.namespace.trim().is_empty() || point.type_name.trim().is_empty() {
        return Err(vector_error("point requires namespace and type"));
    }
    if !valid_sha256(&point.record_sha256) || !valid_sha256(&point.input_sha256) {
        return Err(vector_error("point requires lowercase SHA-256 digests"));
    }
    validate_vector(&point.vector, dimensions)
}

pub(crate) fn validate_descriptor(descriptor: &EmbeddingDescriptor) -> Result<(), Error> {
    if descriptor.model_id.trim().is_empty()
        || descriptor.input_schema != INPUT_SCHEMA
        || !valid_sha256(&descriptor.model_sha256)
        || !valid_sha256(&descriptor.tokenizer_sha256)
        || !(1..=65_536).contains(&descriptor.dimensions)
    {
        return Err(vector_error("invalid embedding descriptor"));
    }
    Ok(())
}

pub(crate) fn validate_search(request: &VectorSearchRequest, dimensions: u64) -> Result<(), Error> {
    if !(1..=100).contains(&request.limit) {
        return Err(vector_error("search limit must be between 1 and 100"));
    }
    validate_vector(&request.vector, dimensions)
}

pub(crate) fn validate_vector(vector: &[f32], dimensions: u64) -> Result<(), Error> {
    if vector.len() != dimensions as usize {
        return Err(vector_error("embedding dimensions do not match config"));
    }
    if vector.iter().any(|value| !value.is_finite()) {
        return Err(vector_error("embedding contains a non-finite value"));
    }
    Ok(())
}

pub(crate) fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(crate) fn valid_collection_name(value: &str) -> bool {
    (1..=200).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub(crate) fn vector_error(reason: &str) -> Error {
    Error::Projection(format!("vector qdrant: {reason}"))
}
