//! What this provider sends, what it accepts back, and what it says when the
//! answer is wrong — all without a socket.
//!
//! The transport is stood in for, so these are statements about the provider's
//! rules rather than about the network. What is deliberately NOT covered here
//! is the socket configuration itself: no test in this file proves that HTTPS
//! is enforced, that redirects error, or that the environment's proxy is
//! ignored. Those live in `transport.rs`, which is configuration and no
//! decisions, and are verified by reading it.
mod key;
mod request;
mod response;

use super::super::super::super::config::{
    EmbeddingConfig, VectorConfig, VoyageEmbeddingConfig, VoyageProvider,
};
use super::transport::Post;
use super::voyage::{VoyageRuntime, for_tests};
use crate::kernel::error::Error;
use crate::vector::model::{DistanceMetric, EmbeddingDocument, INPUT_SCHEMA};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use uuid::Uuid;

/// One recorded call: the key the provider chose to send, and the body it built.
pub(super) struct Call {
    pub(super) key: String,
    pub(super) body: serde_json::Value,
}

/// A transport that answers from a script and remembers what it was asked.
pub(super) struct Recorder {
    calls: Mutex<Vec<Call>>,
    answers: Mutex<Vec<Result<(u16, String), Error>>>,
}

impl Recorder {
    pub(super) fn new(answers: Vec<Result<(u16, String), Error>>) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            calls: Mutex::new(Vec::new()),
            answers: Mutex::new(answers.into_iter().rev().collect()),
        })
    }

    pub(super) fn calls(&self) -> std::sync::MutexGuard<'_, Vec<Call>> {
        self.calls.lock().expect("calls")
    }
}

impl Post for std::sync::Arc<Recorder> {
    fn post_json(&self, key: &str, body: &str) -> Result<(u16, String), Error> {
        self.calls().push(Call {
            key: key.to_owned(),
            body: serde_json::from_str(body).expect("the provider sent invalid JSON"),
        });
        self.answers
            .lock()
            .expect("answers")
            .pop()
            .expect("the provider made more calls than the test scripted")
    }
}

/// A body Voyage would send back for `count` inputs, in order.
pub(super) fn ok_body(count: usize) -> String {
    let data = (0..count)
        .map(|index| serde_json::json!({ "embedding": vector(1.0), "index": index }))
        .collect::<Vec<_>>();
    serde_json::json!({ "object": "list", "model": "voyage-4-large", "data": data }).to_string()
}

pub(super) fn vector(value: f32) -> Vec<f32> {
    vec![value; 2048]
}

pub(super) fn ok(count: usize) -> Result<(u16, String), Error> {
    Ok((200, ok_body(count)))
}

pub(super) fn documents(count: usize) -> Vec<EmbeddingDocument> {
    (0..count)
        .map(|index| EmbeddingDocument {
            record_id: Uuid::now_v7(),
            namespace: "agent.memory".into(),
            type_name: "agent.lesson.v1".into(),
            record_sha256: "0".repeat(64),
            input_sha256: "1".repeat(64),
            text: format!("document {index}"),
        })
        .collect()
}

/// A store directory holding a key file, so the runtime has something to read.
pub(super) fn key_file(name: &str, contents: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "equill-voyage-{name}-{}-{}",
        std::process::id(),
        Uuid::now_v7()
    ));
    std::fs::create_dir_all(&directory).expect("key directory");
    let path = directory.join("token");
    std::fs::write(&path, contents).expect("key file");
    path
}

pub(super) fn config() -> VectorConfig {
    VectorConfig {
        embed_types: Vec::new(),
        schema: "equill.qdrant-config.v1".into(),
        enabled: true,
        endpoint: "http://127.0.0.1:9".into(),
        collection_alias: "equill_voyage_test".into(),
        store_id: Uuid::now_v7(),
        dimensions: 2048,
        distance: DistanceMetric::Cosine,
        embedding: EmbeddingConfig::Voyage(embedding(Path::new("/absolute/token"))),
        api_key_env: None,
        allow_remote: true,
    }
}

pub(super) fn embedding(api_key_file: &Path) -> VoyageEmbeddingConfig {
    VoyageEmbeddingConfig {
        provider: VoyageProvider::Voyage,
        model_id: "voyage-4-large".into(),
        input_schema: INPUT_SCHEMA.into(),
        api_key_file: api_key_file.to_path_buf(),
        allow_remote: true,
    }
}

pub(super) fn runtime(
    key: &Path,
    answers: Vec<Result<(u16, String), Error>>,
) -> (VoyageRuntime, std::sync::Arc<Recorder>) {
    let recorder = Recorder::new(answers);
    let runtime = for_tests(&config(), &embedding(key), Box::new(recorder.clone()))
        .expect("the runtime refused a configuration the config reader accepts");
    (runtime, recorder)
}
