use super::super::embedding::{EMBED_MODEL_ID, VECTOR_DIMENSIONS};
use super::super::{SearchStrategy, configure, rebuild, search};
use crate::command::init;
use crate::kernel::digest::sha256_hex;
use crate::projection::SearchRequest;
use crate::record::{RecordDraft, append};
use crate::schema::{self, TypeDefinition};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// The only test that needs a live Qdrant and real weights, so it is gated on
/// both and skipped otherwise. It is the end-to-end claim in one place: a
/// ledger becomes embeddings, the alias is activated, the store reports Ready,
/// and a paraphrased question finds the record that answers it — through the
/// same verification path an ordinary caller uses.
#[test]
fn endpoint_gated_rebuild_then_semantic_answer() {
    let (Some(endpoint), Some(artifacts)) = (endpoint(), artifacts()) else {
        return;
    };
    let root = store("e2e");
    let target = add(
        &root,
        "Always run the build checks before merging a change.",
    );
    add(&root, "Rotate credentials after every incident review.");
    add(
        &root,
        "Prefer small reversible deployments over big releases.",
    );
    let file = root.join("vector.json");
    fs::write(&file, config(&endpoint, &artifacts)).expect("config file");

    configure(&root, &file, "owner").expect("configure");
    let report = rebuild(&root, "owner").expect("rebuild");
    let answer = search(
        &root,
        &SearchRequest {
            query: "how do I check a change before merging".into(),
            namespace: None,
            type_name: None,
            limit: 3,
        },
        SearchStrategy::Vector,
    )
    .expect("semantic search");

    assert_eq!(report.records, 3);
    assert_eq!(answer.answered_by, "vector");
    assert!(answer.fallback.is_none());
    assert!(answer.rejected.is_empty(), "{:?}", answer.rejected);
    assert_eq!(
        answer.hits.first().map(|hit| hit.record.id),
        Some(target),
        "the paraphrase must rank the matching lesson first"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

fn endpoint() -> Option<String> {
    std::env::var("EQUILL_QDRANT_ENDPOINT").ok()
}

fn artifacts() -> Option<PathBuf> {
    let directory = PathBuf::from(std::env::var("EQUILL_VECTOR_ARTIFACTS").ok()?);
    directory
        .join("model.safetensors")
        .is_file()
        .then_some(directory)
}

fn config(endpoint: &str, artifacts: &Path) -> Vec<u8> {
    let artifact = |name: &str| {
        json!({
            "path": artifacts.join(name),
            "sha256": sha256_hex(&fs::read(artifacts.join(name)).expect("artifact"))
        })
    };
    serde_json::to_vec_pretty(&json!({
        "schema": "equill.qdrant-config.v1",
        "enabled": true,
        "endpoint": endpoint,
        "collection_alias": format!("equill_e2e_{}", Uuid::now_v7().simple()),
        "store_id": Uuid::now_v7(),
        "dimensions": VECTOR_DIMENSIONS,
        "distance": "cosine",
        "embedding": {
            "model_id": EMBED_MODEL_ID,
            "input_schema": "equill.record.embedding.v1",
            "model": artifact("model.safetensors"),
            "tokenizer": artifact("tokenizer.json"),
            "config": artifact("config.json")
        }
    }))
    .expect("config json")
}

fn store(name: &str) -> PathBuf {
    let root = super::support::root(name);
    init::create(&root, "owner", "agent.memory").expect("initialize");
    schema::register(
        &root,
        TypeDefinition {
            type_name: "agent.lesson.v1".into(),
            uri: "equill://agent.lesson/v1".into(),
            owner: "owner".into(),
            payload_schema: json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": { "rule": { "type": "string" } },
                "required": ["rule"],
                "additionalProperties": false
            }),
            lifecycle: Default::default(),
        },
        "owner",
    )
    .expect("register schema");
    root
}

fn add(root: &Path, rule: &str) -> Uuid {
    append(
        root,
        RecordDraft {
            namespace: "agent.memory".into(),
            type_name: "agent.lesson.v1".into(),
            observed_at: "2026-01-01T00:00:00Z".into(),
            valid_at: None,
            payload: json!({ "rule": rule }),
            evidence: Vec::new(),
            tags: Vec::new(),
            supersedes: None,
        },
        "owner",
    )
    .expect("append")
    .id
}
