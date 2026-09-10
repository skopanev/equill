use super::super::{VectorProjection, VectorState, state};
use super::support;
use crate::command::status;
use std::fs;

#[test]
fn absent_config_is_disabled_without_constructing_a_client() {
    let root = support::root("absent");
    fs::create_dir_all(&root).expect("test root");

    assert_eq!(state(&root).expect("state"), VectorState::Disabled);
    assert!(VectorProjection::open(&root).expect("open").is_none());

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn status_reads_absent_config_without_network() {
    let root = support::root("status");
    crate::command::init::create(&root, "test-owner", "agent.memory").expect("store");

    let report =
        serde_json::to_value(status::report(Some(&root)).expect("status")).expect("status JSON");

    assert_eq!(report["components"][3]["state"], "disabled");
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn enabled_local_config_builds_a_lazy_client_and_stays_missing() {
    let root = support::root("local");
    let value = support::config(&root);
    support::write(&root, &value);

    assert_eq!(state(&root).expect("state"), VectorState::Missing);
    assert!(VectorProjection::open(&root).expect("open").is_some());

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn remote_endpoint_requires_explicit_tls_opt_in() {
    let root = support::root("remote");
    let mut value = support::config(&root);
    value["endpoint"] = "http://qdrant.invalid:6334".into();
    support::write(&root, &value);

    let error = state(&root).expect_err("reject remote HTTP");
    assert!(error.to_string().contains("explicit TLS opt-in"));

    value["endpoint"] = "https://qdrant.invalid:6334".into();
    value["allow_remote"] = true.into();
    support::write(&root, &value);
    assert_eq!(state(&root).expect("explicit remote"), VectorState::Missing);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn artifact_hash_mismatch_is_sanitized() {
    let root = support::root("artifact");
    let mut value = support::config(&root);
    value["embedding"]["model"]["sha256"] = "0".repeat(64).into();
    support::write(&root, &value);

    let error = state(&root).expect_err("reject model mismatch").to_string();

    assert!(error.contains("model artifact hash mismatch"));
    assert!(!error.contains("model.onnx"));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn config_serialization_never_contains_an_api_key_value() {
    let root = support::root("secret");
    let mut value = support::config(&root);
    value["api_key_env"] = "EQUILL_QDRANT_TEST_KEY".into();
    support::write(&root, &value);

    let loaded = super::super::config::load(&root)
        .expect("load")
        .expect("config");
    let serialized = serde_json::to_string(&loaded).expect("serialize");

    assert!(serialized.contains("EQUILL_QDRANT_TEST_KEY"));
    assert!(!serialized.contains("secret-value"));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn ollama_config_pins_a_local_model_without_artifact_files() {
    let root = support::root("ollama");
    let mut value = support::config(&root);
    value["dimensions"] = 4096.into();
    value["embedding"] = serde_json::json!({
        "provider": "ollama",
        "endpoint": "http://127.0.0.1:11434",
        "model_id": "qwen3-embedding:8b-q8_0",
        "model_sha256": "a".repeat(64),
        "input_schema": "equill.record.embedding.v1"
    });
    support::write(&root, &value);

    assert_eq!(state(&root).expect("state"), VectorState::Missing);
    let loaded = super::super::config::load(&root)
        .expect("load")
        .expect("config");
    assert_eq!(loaded.embedding.model_sha256(), "a".repeat(64));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn ollama_provider_is_loopback_only() {
    let root = support::root("ollama-remote");
    let mut value = support::config(&root);
    value["embedding"] = serde_json::json!({
        "provider": "ollama",
        "endpoint": "https://ollama.invalid:11434",
        "model_id": "qwen3-embedding:8b-q8_0",
        "model_sha256": "a".repeat(64),
        "input_schema": "equill.record.embedding.v1"
    });
    support::write(&root, &value);

    let error = state(&root).expect_err("reject remote ollama").to_string();
    assert!(error.contains("remote endpoint requires explicit TLS opt-in"));
    fs::remove_dir_all(root).expect("cleanup");
}

/// A descriptor written before the limits existed keeps loading, and reads as
/// the default rather than as "no limit". An unbounded input is what a paid
/// provider bills for and what a context window refuses, so absence cannot
/// mean unbounded.
#[test]
fn absent_limits_read_as_the_default_and_explicit_ones_are_honoured() {
    let root = support::root("limits-default");
    let descriptor = support::config(&root);
    assert!(
        descriptor.get("max_query_chars").is_none(),
        "the fixture already names the limits, so this measures nothing"
    );
    support::write(&root, &descriptor);

    let loaded = crate::vector::config::load(&root)
        .expect("config")
        .expect("configured");

    assert_eq!(loaded.max_query_chars, crate::vector::DEFAULT_MAX_CHARS);
    assert_eq!(loaded.max_document_chars, crate::vector::DEFAULT_MAX_CHARS);
    assert_eq!(crate::vector::DEFAULT_MAX_CHARS, 2_000);

    let mut explicit = support::config(&root);
    explicit["max_query_chars"] = serde_json::json!(512);
    explicit["max_document_chars"] = serde_json::json!(4_096);
    support::write(&root, &explicit);
    let overridden = crate::vector::config::load(&root)
        .expect("config")
        .expect("configured");
    assert_eq!(overridden.max_query_chars, 512);
    assert_eq!(overridden.max_document_chars, 4_096);
    fs::remove_dir_all(root).expect("cleanup");
}

/// Zero is refused rather than read as "no limit": a silent zero would embed
/// nothing and look like a configuration that worked.
#[test]
fn a_zero_or_absurd_limit_is_refused_at_load() {
    let root = support::root("limits-invalid");
    for (field, value) in [
        ("max_query_chars", 0_u64),
        ("max_document_chars", 0),
        ("max_query_chars", 10_000_000),
    ] {
        let mut descriptor = support::config(&root);
        descriptor[field] = serde_json::json!(value);
        support::write(&root, &descriptor);

        let error =
            crate::vector::config::load(&root).expect_err(&format!("{field}={value} was accepted"));

        assert!(error.to_string().contains(field), "{field}: {error}");
    }
    fs::remove_dir_all(root).expect("cleanup");
}
