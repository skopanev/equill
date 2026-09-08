use super::qdrant::{ProviderHit, ProviderMetadata, ProviderPoint};
use crate::kernel::error::Error;
use crate::vector::model::vector_error;
use crate::vector::model::{EmbeddingDocument, VectorPoint};
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
    let payload = qdrant_payload(point)?;
    Ok(api::PointStruct::new(
        physical_id(point.store_id, point.point.record_id),
        point.point.vector.clone(),
        payload,
    ))
}

/// The bookkeeping half of a point, shared by the two ways of writing it.
pub(super) fn qdrant_payload(point: &ProviderPoint) -> Result<Payload, Error> {
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
    serde_json::to_value(payload)
        .ok()
        .and_then(|value| Payload::try_from(value).ok())
        .ok_or_else(|| vector_error("point metadata conversion failed"))
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

/// Writing a point's bookkeeping without its vector.
///
/// Kept beside the point mapping rather than with the collection's other
/// writes, because that is what it is: the same payload construction as an
/// upsert, with the expensive half left out.
impl<T: super::qdrant::Transport> super::collection::Collection<T> {
    /// The bookkeeping half of an upsert: same point, same vector, new hashes.
    /// Re-running the model to write a hash it never reads would cost the whole
    /// corpus for a field outside its input.
    pub(crate) fn relabel(
        &self,
        physical: &str,
        documents: &[EmbeddingDocument],
    ) -> Result<(), Error> {
        let points = documents
            .iter()
            .map(|item| self.bookkeeping(item))
            .collect::<Vec<_>>();
        self.transport.set_payload(physical, &points)
    }

    fn bookkeeping(&self, document: &EmbeddingDocument) -> ProviderPoint {
        ProviderPoint {
            store_id: self.config.store_id,
            model_sha256: self.config.embedding.model_sha256().to_owned(),
            point: VectorPoint {
                record_id: document.record_id,
                namespace: document.namespace.clone(),
                type_name: document.type_name.clone(),
                record_sha256: document.record_sha256.clone(),
                input_sha256: document.input_sha256.clone(),
                vector: Vec::new(),
            },
        }
    }
}

/// The payload write itself, kept here with the payload construction it uses.
pub(super) fn write_payload(
    transport: &super::qdrant::QdrantTransport,
    collection: &str,
    points: &[ProviderPoint],
) -> Result<(), Error> {
    for point in points {
        let id = super::point::physical_id(point.store_id, point.point.record_id);
        let payload = super::point::qdrant_payload(point)?;
        let request = api::SetPayloadPointsBuilder::new(collection, payload)
            .points_selector(api::PointsIdsList {
                ids: vec![id.into()],
            })
            .wait(true);
        transport.run("set point payload", move |client| async move {
            client.set_payload(request).await
        })?;
    }
    Ok(())
}
