use super::qdrant::CollectionSchema;
use crate::vector::config::{EmbeddingConfig, ModelArtifact, VectorConfig};
use crate::vector::model::{DistanceMetric, VectorPoint, VectorSearchRequest};
use std::path::PathBuf;
use uuid::Uuid;

pub(super) fn config() -> VectorConfig {
    VectorConfig {
        schema: "equill.qdrant-config.v1".into(),
        enabled: true,
        endpoint: "http://127.0.0.1:9".into(),
        collection_alias: "equill_records_test".into(),
        store_id: Uuid::now_v7(),
        dimensions: 3,
        distance: DistanceMetric::Cosine,
        embedding: EmbeddingConfig {
            model_id: "test-only".into(),
            input_schema: "equill.record.embedding.v1".into(),
            model: artifact("a"),
            tokenizer: artifact("b"),
            model_config: artifact("c"),
        },
        api_key_env: None,
        allow_remote: false,
    }
}

pub(super) fn schema(config: &VectorConfig) -> CollectionSchema {
    CollectionSchema {
        dimensions: config.dimensions,
        distance: config.distance,
        store_id: config.store_id,
        model_sha256: config.embedding.model.sha256.clone(),
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
