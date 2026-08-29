use crate::kernel::digest::sha256_hex;
use crate::vector::VectorProgress;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use uuid::Uuid;

static NEXT: AtomicU64 = AtomicU64::new(1);

pub fn root(name: &str) -> PathBuf {
    let suffix = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "equill-vector-{name}-{}-{suffix}",
        std::process::id()
    ))
}

pub fn config(root: &Path) -> Value {
    let models = root.join("models");
    fs::create_dir_all(&models).expect("model directory");
    fs::write(models.join("model.onnx"), b"synthetic model").expect("model fixture");
    fs::write(models.join("tokenizer.json"), b"synthetic tokenizer").expect("tokenizer fixture");
    // Follows the config.rs edit: the third artifact is hash-verified too.
    fs::write(models.join("config.json"), b"synthetic model config").expect("config fixture");
    json!({
        "schema": "equill.qdrant-config.v1",
        "enabled": true,
        "endpoint": "http://127.0.0.1:9",
        "collection_alias": "equill_records_test",
        "store_id": Uuid::now_v7(),
        "dimensions": 3,
        "distance": "cosine",
        "embedding": {
            "model_id": "synthetic-embedding-v1",
            "input_schema": "equill.record.embedding.v1",
            "model": {
                "path": "models/model.onnx",
                "sha256": sha256_hex(b"synthetic model")
            },
            "tokenizer": {
                "path": "models/tokenizer.json",
                "sha256": sha256_hex(b"synthetic tokenizer")
            },
            "config": {
                "path": "models/config.json",
                "sha256": sha256_hex(b"synthetic model config")
            }
        }
    })
}

pub fn write(root: &Path, value: &Value) {
    let directory = root.join("registry/vector");
    fs::create_dir_all(&directory).expect("config directory");
    fs::write(
        directory.join("qdrant.json"),
        serde_json::to_vec(value).expect("config JSON"),
    )
    .expect("write config");
}

pub fn sync_events(physical: &str, digest: &str, pending: usize) -> Vec<VectorProgress> {
    let mut events = vec![VectorProgress::Scanned {
        collection: physical.into(),
        records: 1,
        pending,
        corpus_sha256: digest.into(),
    }];
    if pending > 0 {
        events.extend([
            VectorProgress::LoadingModel,
            VectorProgress::Embedded {
                completed: 1,
                total: 1,
            },
            VectorProgress::Upserted {
                completed: 1,
                total: 1,
            },
        ]);
    }
    events.push(VectorProgress::Ready {
        collection: physical.into(),
        corpus_sha256: digest.into(),
    });
    events
}
