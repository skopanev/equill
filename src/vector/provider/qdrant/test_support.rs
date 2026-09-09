use super::point::physical_id;
use super::qdrant::{
    CollectionSchema, ProviderHit, ProviderMetadata, ProviderPoint, Query, Transport,
};
use crate::kernel::error::Error;
use crate::vector::config::{EmbeddingConfig, LocalEmbeddingConfig, ModelArtifact, VectorConfig};
use crate::vector::model::{DistanceMetric, VectorPoint, VectorSearchRequest};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub(super) fn config() -> VectorConfig {
    VectorConfig {
        embed_types: Vec::new(),
        schema: "equill.qdrant-config.v1".into(),
        enabled: true,
        endpoint: "http://127.0.0.1:9".into(),
        collection_alias: "equill_records_test".into(),
        store_id: Uuid::now_v7(),
        dimensions: 3,
        distance: DistanceMetric::Cosine,
        embedding: EmbeddingConfig::Local(LocalEmbeddingConfig {
            model_id: "test-only".into(),
            input_schema: "equill.record.embedding.v1".into(),
            model: artifact("a"),
            tokenizer: artifact("b"),
            model_config: artifact("c"),
        }),
        api_key_env: None,
        allow_remote: false,
    }
}

pub(super) fn schema(config: &VectorConfig) -> CollectionSchema {
    CollectionSchema {
        dimensions: config.dimensions,
        distance: config.distance,
        store_id: config.store_id,
        model_sha256: config.embedding.model_sha256().to_owned(),
    }
}

pub(super) fn point() -> VectorPoint {
    VectorPoint {
        record_id: Uuid::now_v7(),
        namespace: "agent.memory".into(),
        type_name: "agent.lesson.v1".into(),
        record_sha256: "c".repeat(64),
        input_sha256: "d".repeat(64),
        vector: vec![0.1, 0.2, 0.3],
    }
}

pub(super) fn search() -> VectorSearchRequest {
    VectorSearchRequest {
        vector: vec![0.3, 0.2, 0.1],
        query_instruction: crate::retrieval::DEFAULT_QUERY_INSTRUCTION.into(),
        score_threshold: None,
        namespaces: vec!["agent.memory".into()],
        type_names: vec!["agent.lesson.v1".into()],
        limit: 10,
    }
}

fn artifact(value: &str) -> ModelArtifact {
    ModelArtifact {
        path: PathBuf::from("test-only"),
        sha256: value.repeat(64),
    }
}

#[derive(Clone, Default)]
pub(super) struct FakeTransport {
    pub(super) inner: Arc<Mutex<FakeState>>,
}
#[derive(Default)]
pub(super) struct FakeState {
    pub(super) schemas: HashMap<String, CollectionSchema>,
    pub(super) aliases: HashMap<String, String>,
    pub(super) points: Vec<ProviderPoint>,
    pub(super) hits: Vec<ProviderHit>,
    queries: usize,
    pub(super) fail_retarget: bool,
}

impl Transport for FakeTransport {
    fn set_payload(&self, _c: &str, _p: &[ProviderPoint]) -> Result<(), Error> {
        Ok(())
    }

    fn collection_schema(&self, name: &str) -> Result<Option<CollectionSchema>, Error> {
        let state = self.inner.lock().unwrap();
        let physical = state.aliases.get(name).map(String::as_str).unwrap_or(name);
        Ok(state.schemas.get(physical).cloned())
    }

    fn create_collection(&self, name: &str, schema: CollectionSchema) -> Result<(), Error> {
        self.inner
            .lock()
            .unwrap()
            .schemas
            .insert(name.into(), schema);
        Ok(())
    }

    fn upsert(&self, _collection: &str, points: &[ProviderPoint]) -> Result<(), Error> {
        self.inner.lock().unwrap().points.extend_from_slice(points);
        Ok(())
    }

    fn delete(&self, _collection: &str, point_ids: &[Uuid]) -> Result<(), Error> {
        self.inner
            .lock()
            .unwrap()
            .points
            .retain(|item| !point_ids.contains(&physical_id(item.store_id, item.point.record_id)));
        Ok(())
    }

    fn metadata(
        &self,
        _collection: &str,
        point_ids: &[Uuid],
    ) -> Result<Vec<ProviderMetadata>, Error> {
        let state = self.inner.lock().unwrap();
        Ok(state
            .points
            .iter()
            .filter(|item| point_ids.contains(&physical_id(item.store_id, item.point.record_id)))
            .map(|item| ProviderMetadata {
                store_id: item.store_id,
                model_sha256: item.model_sha256.clone(),
                record_id: item.point.record_id,
                record_sha256: item.point.record_sha256.clone(),
                input_sha256: item.point.input_sha256.clone(),
            })
            .collect())
    }

    fn query(&self, _query: Query) -> Result<Vec<ProviderHit>, Error> {
        let mut state = self.inner.lock().unwrap();
        state.queries += 1;
        Ok(state.hits.clone())
    }

    fn alias_target(&self, alias: &str) -> Result<Option<String>, Error> {
        Ok(self.inner.lock().unwrap().aliases.get(alias).cloned())
    }

    fn retarget_alias(
        &self,
        alias: &str,
        previous: Option<&str>,
        target: Option<&str>,
    ) -> Result<(), Error> {
        let mut state = self.inner.lock().unwrap();
        if state.fail_retarget || state.aliases.get(alias).map(String::as_str) != previous {
            return Err(crate::vector::model::vector_error("retarget alias failed"));
        }
        match target {
            Some(collection) => {
                state.aliases.insert(alias.into(), collection.into());
            }
            None => {
                state.aliases.remove(alias);
            }
        }
        Ok(())
    }
}
