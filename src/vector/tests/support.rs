use crate::kernel::digest::sha256_hex;
use crate::projection::SearchRequest;
use crate::record::StoredRecord;
use crate::vector::RejectedHit;
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
/// Freshness is read from published markers, not from the substituted half and
/// not from the ledger, so staging a lagging index means staging both sides of
/// that comparison: a target the ledger published and a checkpoint that covers
/// `indexed` records at a revision `pending` writes behind it. A bare store
/// with no markers at all would honestly report `Unknown`, and a receipt
/// asserting on those would be asserting that nothing was configured.
pub(crate) fn stage_lagging_index(root: &std::path::Path, indexed: usize, pending: usize) {
    let config = config(root);
    write(root, &config);
    let directory = root.join("projections/qdrant");
    fs::create_dir_all(&directory).expect("marker directory");
    crate::vector::desired::publish(root, (indexed + pending) as u64).expect("publish target");
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
            "indexed_revision": indexed,
            // A digest of something, and deliberately not of this corpus: the
            // difference is what makes the checkpoint behind rather than level.
            "indexed_sha256": "f".repeat(64),
        }))
        .expect("marker JSON"),
    )
    .expect("write marker");
}

/// Configure the index and publish a checkpoint that says it is level with the
/// store's published target.
///
/// Markers only: no provider, no ledger read. The revision is whatever the
/// store actually published, so the answer this stages is `Current` whether the
/// fixture appended before or after the config landed.
pub(crate) fn stage_current_index(root: &std::path::Path, indexed: usize) {
    let config = config(root);
    write(root, &config);
    let directory = root.join("projections/qdrant");
    fs::create_dir_all(&directory).expect("marker directory");
    let revision = crate::vector::desired::read(root)
        .expect("desired marker readable")
        .map_or(0, |target| target.revision);
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
            "indexed_revision": revision,
            // A digest of something: freshness compares revisions, and only the
            // format of this field is checked on the request path.
            "indexed_sha256": "a".repeat(64),
        }))
        .expect("marker JSON"),
    )
    .expect("write marker");
}

pub fn cli(root: &std::path::Path) -> Result<String, crate::kernel::error::Error> {
    crate::command::query::search(
        true,
        root.to_path_buf(),
        Some("deployment".into()),
        None,
        None,
        10,
        None,
        Vec::new(),
        false,
        crate::command::cli::FormatArg::Jsonl,
        Vec::new(),
        false,
    )
}

/// A second substitute that names a different record, so the two are told
/// apart by the answer rather than by which function was installed.
pub fn first_only(
    store_root: &std::path::Path,
    _request: &SearchRequest,
) -> Result<(Vec<StoredRecord>, Vec<RejectedHit>), crate::kernel::error::Error> {
    let mut records = crate::record::read_all(store_root)?;
    records.truncate(1);
    Ok((records, Vec::new()))
}

pub fn request() -> SearchRequest {
    SearchRequest {
        query: Some("deployment".into()),
        namespace: None,
        type_name: None,
        limit: 10,
    }
}

pub fn failing(
    _store_root: &std::path::Path,
    _request: &SearchRequest,
) -> Result<(Vec<StoredRecord>, Vec<crate::vector::RejectedHit>), crate::kernel::error::Error> {
    Err(crate::vector::model::vector_error(
        "index unreachable in this test",
    ))
}

pub fn half(
    store_root: &std::path::Path,
    _request: &SearchRequest,
) -> Result<(Vec<StoredRecord>, Vec<crate::vector::RejectedHit>), crate::kernel::error::Error> {
    Ok((staged(store_root), Vec::new()))
}

/// The substitute names ONE record, and text will find all of them.
///
/// A half that returned everything would be indistinguishable from no merge at
/// all: the counts would match whether the text list was consulted or quietly
/// dropped. With a strict subset, only a real union reaches the full count, and
/// the one record both halves name is the one whose position proves the
/// fusion — agreement has to outrank a record that leads only the text list.
pub fn staged(store_root: &std::path::Path) -> Vec<StoredRecord> {
    let mut records = crate::record::read_all(store_root).expect("ledger");
    records.split_off(records.len() - 1)
}
