//! Building the store a real `equill` binary will accept: a schema it can
//! validate against, and a projection pointed somewhere harmless.
use std::path::Path;

/// Points the projection at a port nothing listens on. Artifacts are synthetic
/// files with honest digests, so config validation passes and no model is ever
/// loaded.
pub fn configure_endpoint(root: &Path, endpoint: &str) {
    let path = root.join("registry/vector/qdrant.json");
    let mut config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("config")).expect("json");
    config["endpoint"] = serde_json::Value::String(endpoint.to_owned());
    write_json(&path, &config);
}

pub fn configure(root: &Path) {
    let models = root.join("models");
    std::fs::create_dir_all(&models).expect("models");
    let artifact = |name: &str, body: &str| {
        std::fs::write(models.join(name), body).expect("artifact");
        serde_json::json!({
            "path": format!("models/{name}"),
            "sha256": sha256(body.as_bytes())
        })
    };
    let config = serde_json::json!({
        "schema": "equill.qdrant-config.v1",
        "enabled": true,
        "endpoint": "http://127.0.0.1:1",
        "collection_alias": "equill_process_test",
        "store_id": uuid::Uuid::now_v7(),
        "dimensions": 1024,
        "distance": "cosine",
        "embedding": {
            "model_id": "Qwen/Qwen3-Embedding-0.6B",
            "input_schema": "equill.record.embedding.v1",
            "model": artifact("model.safetensors", "synthetic weights"),
            "tokenizer": artifact("tokenizer.json", "synthetic tokenizer"),
            "config": artifact("config.json", "synthetic config")
        }
    });
    std::fs::create_dir_all(root.join("registry/vector")).expect("registry");
    write_json(&root.join("registry/vector/qdrant.json"), &config);
}

pub fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn write_json(path: &Path, value: &serde_json::Value) {
    std::fs::write(path, serde_json::to_vec_pretty(value).expect("json")).expect("write");
}

pub fn write_line(path: &Path, value: &serde_json::Value) {
    std::fs::write(path, serde_json::to_vec(value).expect("json")).expect("write");
}

/// A profile and selector wide enough to return the seeded records: one type,
/// no required tags, so a context read measures assembly rather than an empty
/// result.
pub fn register_reader_profile(root: &Path) {
    register(
        root,
        "selector",
        "selector.json",
        &serde_json::json!({
            "id": "reader.lesson",
            "version": "1",
            "type": "agent.lesson.v1",
            "strategies": ["fts"],
            "required_tags": [],
            "core_tags": [],
            "rank_pointer": "/confidence"
        }),
    );
    register(
        root,
        "profile",
        "profile.json",
        &serde_json::json!({
            "id": "reader",
            "version": "1",
            "actors": ["*"],
            "grants": [{ "namespace": "agent.memory", "types": ["agent.lesson.v1"] }],
            "selectors": ["reader.lesson"],
            "budget": {}
        }),
    );
}

fn register(root: &Path, command: &str, name: &str, body: &serde_json::Value) {
    let path = root.join(name);
    write_json(&path, body);
    let out = std::process::Command::new(super::binary())
        .args([command, "register", "--file"])
        .arg(&path)
        .arg("--store")
        .arg(root)
        .env("EQUILL_ACTOR", "owner")
        .output()
        .expect("register");
    assert!(
        out.status.success(),
        "{command} register failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
