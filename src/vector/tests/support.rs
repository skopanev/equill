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

/// Leave the store with a configured index whose checkpoint is real but stale.
///
/// Freshness is read from the store, not from the substituted half, so a staged
/// hybrid answer over a bare store would honestly report `Disabled` and no
/// counts — and a receipt asserting on those would be asserting that nothing
/// was configured. This writes a checkpoint that describes this store, this
/// alias and this model, and covers fewer records than the ledger holds.
pub(crate) fn stage_lagging_index(root: &std::path::Path, indexed: usize) {
    let config = config(root);
    write(root, &config);
    let directory = root.join("projections/qdrant");
    fs::create_dir_all(&directory).expect("marker directory");
    fs::write(
        directory.join("state.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema": "equill.qdrant-state.v2",
            "state": "ready",
            "store_id": config["store_id"],
            "collection_alias": config["collection_alias"],
            "physical_collection": "equill_records_test_p0",
            "model_sha256": config["embedding"]["model"]["sha256"],
            "indexed_records": indexed,
            // A digest of something, and deliberately not of this corpus: the
            // difference is what makes the checkpoint behind rather than level.
            "indexed_sha256": "f".repeat(64),
        }))
        .expect("marker JSON"),
    )
    .expect("write marker");
}
