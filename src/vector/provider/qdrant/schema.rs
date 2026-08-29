use crate::kernel::error::Error;
use crate::vector::model::{DistanceMetric, valid_sha256, vector_error};
use qdrant_client::qdrant as api;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use uuid::Uuid;

const SCHEMA: &str = "equill.qdrant-collection.v1";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CollectionSchema {
    pub dimensions: u64,
    pub distance: DistanceMetric,
    pub store_id: Uuid,
    pub model_sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    schema: String,
    store_id: Uuid,
    model_sha256: String,
}

pub(super) fn metadata(schema: &CollectionSchema) -> HashMap<String, serde_json::Value> {
    HashMap::from([
        ("schema".into(), json!(SCHEMA)),
        ("store_id".into(), json!(schema.store_id)),
        ("model_sha256".into(), json!(schema.model_sha256)),
    ])
}

pub(super) fn parse(response: api::GetCollectionInfoResponse) -> Result<CollectionSchema, Error> {
    let config = response
        .result
        .and_then(|info| info.config)
        .ok_or_else(|| vector_error("collection configuration is missing"))?;
    let metadata: Metadata = serde_json::from_value(serde_json::Value::Object(
        config
            .metadata
            .into_iter()
            .map(|(key, value)| (key, value.into()))
            .collect(),
    ))
    .map_err(|_| vector_error("collection identity metadata is invalid"))?;
    if metadata.schema != SCHEMA || !valid_sha256(&metadata.model_sha256) {
        return Err(vector_error("collection identity metadata is invalid"));
    }
    let vectors = config
        .params
        .and_then(|params| params.vectors_config)
        .and_then(|vectors| vectors.config)
        .ok_or_else(|| vector_error("collection vector parameters are missing"))?;
    let api::vectors_config::Config::Params(params) = vectors else {
        return Err(vector_error("named vectors are not supported"));
    };
    Ok(CollectionSchema {
        dimensions: params.size,
        distance: from_api(params.distance)?,
        store_id: metadata.store_id,
        model_sha256: metadata.model_sha256,
    })
}

pub(super) fn to_api(distance: DistanceMetric) -> api::Distance {
    match distance {
        DistanceMetric::Cosine => api::Distance::Cosine,
        DistanceMetric::Dot => api::Distance::Dot,
        DistanceMetric::Euclid => api::Distance::Euclid,
        DistanceMetric::Manhattan => api::Distance::Manhattan,
    }
}

fn from_api(distance: i32) -> Result<DistanceMetric, Error> {
    match api::Distance::try_from(distance).ok() {
        Some(api::Distance::Cosine) => Ok(DistanceMetric::Cosine),
        Some(api::Distance::Dot) => Ok(DistanceMetric::Dot),
        Some(api::Distance::Euclid) => Ok(DistanceMetric::Euclid),
        Some(api::Distance::Manhattan) => Ok(DistanceMetric::Manhattan),
        Some(api::Distance::UnknownDistance) | None => {
            Err(vector_error("collection distance is unsupported"))
        }
    }
}
