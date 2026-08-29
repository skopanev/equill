use super::qdrant::{ProviderHit, ProviderMetadata, ProviderPoint};
use crate::kernel::error::Error;
use crate::vector::model::vector_error;
use qdrant_client::Payload;
use qdrant_client::qdrant as api;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub(super) const POINT_SCHEMA: &str = "equill.qdrant-point.v1";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PointPayload {
    schema: String,
    store_id: Uuid,
    record_id: Uuid,
    namespace: String,
    #[serde(rename = "type")]
    type_name: String,
    record_sha256: String,
    input_sha256: String,
    model_sha256: String,
}

pub(super) fn qdrant_point(point: &ProviderPoint) -> Result<api::PointStruct, Error> {
    let payload = PointPayload {
        schema: POINT_SCHEMA.into(),
        store_id: point.store_id,
        record_id: point.point.record_id,
        namespace: point.point.namespace.clone(),
        type_name: point.point.type_name.clone(),
        record_sha256: point.point.record_sha256.clone(),
        input_sha256: point.point.input_sha256.clone(),
        model_sha256: point.model_sha256.clone(),
    };
    let payload = serde_json::to_value(payload)
        .ok()
        .and_then(|value| Payload::try_from(value).ok())
        .ok_or_else(|| vector_error("point metadata conversion failed"))?;
    Ok(api::PointStruct::new(
        physical_id(point.store_id, point.point.record_id),
        point.point.vector.clone(),
        payload,
    ))
}

pub(super) fn provider_hit(point: api::ScoredPoint) -> Result<ProviderHit, Error> {
    let payload = payload(point.id, point.payload, "query")?;
    Ok(ProviderHit {
        store_id: payload.store_id,
        model_sha256: payload.model_sha256,
        record_id: payload.record_id,
        score: point.score,
        record_sha256: payload.record_sha256,
        input_sha256: payload.input_sha256,
    })
}

pub(super) fn provider_metadata(point: api::RetrievedPoint) -> Result<ProviderMetadata, Error> {
    let payload = payload(point.id, point.payload, "retrieval")?;
    Ok(ProviderMetadata {
        store_id: payload.store_id,
        model_sha256: payload.model_sha256,
        record_id: payload.record_id,
        record_sha256: payload.record_sha256,
        input_sha256: payload.input_sha256,
    })
}

fn payload(
    id: Option<api::PointId>,
    fields: std::collections::HashMap<String, api::Value>,
    action: &str,
) -> Result<PointPayload, Error> {
    let id = id
        .and_then(|id| id.point_id_options)
        .and_then(|id| match id {
            api::point_id::PointIdOptions::Uuid(value) => Uuid::parse_str(&value).ok(),
            api::point_id::PointIdOptions::Num(_) => None,
        })
        .ok_or_else(|| vector_error(&format!("{action} returned an invalid point ID")))?;
    let payload: PointPayload = Payload::from(fields)
        .deserialize()
        .map_err(|_| vector_error(&format!("{action} returned invalid point metadata")))?;
    if payload.schema != POINT_SCHEMA || physical_id(payload.store_id, payload.record_id) != id {
        return Err(vector_error(&format!(
            "{action} returned mismatched point metadata"
        )));
    }
    Ok(payload)
}

pub(super) fn physical_id(store_id: Uuid, record_id: Uuid) -> Uuid {
    Uuid::new_v5(&store_id, record_id.as_bytes())
}
