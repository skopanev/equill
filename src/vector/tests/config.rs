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
