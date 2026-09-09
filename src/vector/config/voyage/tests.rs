use super::*;
use crate::vector::config::{EmbeddingConfig, load};
use serde_json::{Value, json};
use std::fs;
use uuid::Uuid;

fn fixture() -> Value {
    json!({
        "schema": "equill.qdrant-config.v1", "enabled": true,
        "endpoint": "http://127.0.0.1:6334", "collection_alias": "synthetic_voyage",
        "store_id": Uuid::now_v7(), "dimensions": 2048, "distance": "cosine",
        "embedding": {"provider": "voyage", "model_id": "voyage-4-large",
            "input_schema": INPUT_SCHEMA, "allow_remote": true,
            "api_key_file": std::env::temp_dir().join("equill-absent-synthetic-key")}
    })
}

#[test]
fn voyage_config_is_lazy_and_does_not_read_credentials_or_contact_api() {
    let root = std::env::temp_dir().join(format!("equill-voyage-config-{}", Uuid::now_v7()));
    let path = root.join("registry/vector");
    fs::create_dir_all(&path).unwrap();
    fs::write(
        path.join("qdrant.json"),
        serde_json::to_vec(&fixture()).unwrap(),
    )
    .unwrap();
    let config = load(&root).unwrap().unwrap();
    assert!(matches!(config.embedding, EmbeddingConfig::Voyage(_)));
    assert_eq!(
        crate::vector::state(&root).unwrap(),
        crate::vector::VectorState::Missing
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn explicit_embedding_opt_in_is_separate_from_remote_qdrant() {
    let mut raw = fixture();
    raw["allow_remote"] = true.into();
    raw["embedding"]
        .as_object_mut()
        .unwrap()
        .remove("allow_remote");
    let config: VectorConfig = serde_json::from_value(raw).unwrap();
    assert!(
        super::super::validate_shape(&config)
            .unwrap_err()
            .to_string()
            .contains("opt-in")
    );
}

#[test]
fn voyage_rejects_wrong_model_dimensions_metric_and_relative_credential() {
    for (field, value) in [("dimensions", json!(4096)), ("distance", json!("dot"))] {
        let mut raw = fixture();
        raw[field] = value;
        let config: VectorConfig = serde_json::from_value(raw).unwrap();
        assert!(super::super::validate_shape(&config).is_err());
    }
    for (field, value) in [
        ("model_id", json!("unknown-model")),
        ("api_key_file", json!("key.token")),
    ] {
        let mut raw = fixture();
        raw["embedding"][field] = value;
        let config: VectorConfig = serde_json::from_value(raw).unwrap();
        assert!(super::super::validate_shape(&config).is_err());
    }
}

#[test]
fn config_rejects_inline_key_and_endpoint_override() {
    for (field, value) in [
        ("api_key", "synthetic-not-a-secret"),
        ("endpoint", "https://example.invalid"),
    ] {
        let mut raw = fixture();
        raw["embedding"][field] = value.into();
        assert!(serde_json::from_value::<VectorConfig>(raw).is_err());
    }
}

#[test]
fn contract_identity_is_stable_across_credential_rotation_but_tracks_input_schema() {
    let raw = fixture();
    let config: VectorConfig = serde_json::from_value(raw).unwrap();
    let EmbeddingConfig::Voyage(mut embedding) = config.embedding else {
        panic!("voyage")
    };
    let before = embedding.fingerprint();
    assert!(crate::vector::model::valid_sha256(&before));
    embedding.api_key_file = std::env::temp_dir().join("rotated-key");
    assert_eq!(before, embedding.fingerprint());
    embedding.input_schema = "future-input-schema".into();
    assert_ne!(before, embedding.fingerprint());
}
