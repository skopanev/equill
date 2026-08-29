use super::super::config::{EmbeddingConfig, ModelArtifact, VectorConfig};
use super::super::model::DistanceMetric;
use super::super::{Embedder, EmbeddingDocument, INPUT_SCHEMA};
use super::{EMBED_MODEL_ID, EmbeddingRuntime, MAX_TOKENS, QUERY_PREFIX, VECTOR_DIMENSIONS};
use crate::kernel::digest::sha256_hex;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// The contract is pinned deliberately: a silent change to pooling, dimensions,
/// sequence length, or the retrieval instruction would keep every test green
/// while making every stored vector mean something else.
#[test]
fn the_embedding_contract_is_pinned() {
    assert_eq!(EMBED_MODEL_ID, "Qwen/Qwen3-Embedding-0.6B");
    assert_eq!(VECTOR_DIMENSIONS, 1024);
    assert_eq!(MAX_TOKENS, 512);
    assert_eq!(
        QUERY_PREFIX,
        "Instruct: Given a web search query, retrieve relevant passages that answer the query\nQuery:"
    );
}

/// Real model weights are large and licence-bound, so they never enter the
/// repository. This runs only when an operator points EQUILL_VECTOR_ARTIFACTS at
/// a local copy; without it the check is skipped rather than silently faked.
#[test]
fn real_artifacts_produce_a_deterministic_and_semantic_embedding() {
    let Some(directory) = artifacts() else {
        return;
    };
    let config = config(&directory);
    let embedder = EmbeddingRuntime::load(Path::new("/"), &config).expect("load local artifacts");

    let lesson = "Run the build checks before merging.";
    let first = embedder.embed(&[document(lesson)]).expect("embed");
    // A decoder carries state between calls, so the claim under test is not
    // "twice in a row agrees" but "the answer does not depend on what came
    // before". Different lengths and both orderings, because a key/value cache
    // is sensitive to exactly that.
    let noise = [
        "The grocery list has apples and bread and a long tail of unrelated words.",
        "Rotate credentials after every incident review.",
        "short",
    ];
    for text in noise {
        embedder.embed(&[document(text)]).expect("noise");
    }
    let second = embedder.embed(&[document(lesson)]).expect("repeat");
    for text in noise.iter().rev() {
        embedder.embed(&[document(text)]).expect("reverse noise");
    }
    let third = embedder
        .embed(&[document(lesson)])
        .expect("repeat reversed");
    let near = embedder
        .embed(&[document("Always run tests prior to merge.")])
        .expect("near");
    let far = embedder
        .embed(&[document("The grocery list has apples and bread.")])
        .expect("far");
    let query = embedder
        .embed_query("how do I verify a change")
        .expect("query");

    assert_eq!(first[0].len(), VECTOR_DIMENSIONS as usize);
    assert_eq!(first, second, "history must not change the vector");
    assert_eq!(first, third, "nor must the order of that history");
    assert!(
        (cosine(&first[0], &first[0]) - 1.0).abs() < 1e-4,
        "L2 normalized"
    );
    assert!(
        cosine(&first[0], &near[0]) > cosine(&first[0], &far[0]) + 0.1,
        "a paraphrase must rank above an unrelated sentence"
    );
    assert_eq!(query.len(), VECTOR_DIMENSIONS as usize);
}

fn artifacts() -> Option<PathBuf> {
    let directory = PathBuf::from(std::env::var("EQUILL_VECTOR_ARTIFACTS").ok()?);
    directory
        .join("model.safetensors")
        .is_file()
        .then_some(directory)
}

fn config(directory: &Path) -> VectorConfig {
    let artifact = |name: &str| ModelArtifact {
        path: directory.join(name),
        sha256: sha256_hex(&std::fs::read(directory.join(name)).expect("artifact")),
    };
    VectorConfig {
        schema: "equill.qdrant-config.v1".into(),
        enabled: true,
        endpoint: "http://127.0.0.1:6333".into(),
        collection_alias: "equill_records_test".into(),
        store_id: Uuid::now_v7(),
        dimensions: VECTOR_DIMENSIONS,
        distance: DistanceMetric::Cosine,
        embedding: EmbeddingConfig {
            model_id: EMBED_MODEL_ID.into(),
            input_schema: INPUT_SCHEMA.into(),
            model: artifact("model.safetensors"),
            tokenizer: artifact("tokenizer.json"),
            model_config: artifact("config.json"),
        },
        api_key_env: None,
        allow_remote: false,
    }
}

fn document(text: &str) -> EmbeddingDocument {
    EmbeddingDocument {
        record_id: Uuid::now_v7(),
        namespace: "agent.memory".into(),
        type_name: "agent.lesson.v1".into(),
        record_sha256: "c".repeat(64),
        input_sha256: "d".repeat(64),
        text: text.into(),
    }
}

fn cosine(left: &[f32], right: &[f32]) -> f32 {
    left.iter().zip(right).map(|(a, b)| a * b).sum()
}
