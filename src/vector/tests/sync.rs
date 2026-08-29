use super::super::model::{
    EmbeddingDescriptor, EmbeddingDocument, VectorPoint, VectorPointMetadata, vector_error,
};
use super::super::operator::{SyncIndex, execute};
use super::super::{Embedder, VectorConfig, VectorState, corpus, state};
use crate::command::init;
use crate::kernel::error::Error;
use crate::record::{RecordDraft, append};
use crate::schema::{self, TypeDefinition};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

const PHYSICAL: &str = "equill_sync_active";

#[derive(Clone)]
struct FakeIndex {
    root: PathBuf,
    config: VectorConfig,
    inner: Arc<Mutex<FakeState>>,
}

#[derive(Default)]
struct FakeState {
    points: HashMap<Uuid, VectorPointMetadata>,
    points_upserted: usize,
    degraded_marks: usize,
    ready_marks: usize,
    fail_upsert: bool,
}

impl SyncIndex for FakeIndex {
    fn active_collection(&self) -> Result<String, Error> {
        Ok(PHYSICAL.into())
    }

    fn metadata(&self, _physical: &str, ids: &[Uuid]) -> Result<Vec<VectorPointMetadata>, Error> {
        let state = self.inner.lock().unwrap();
        Ok(ids
            .iter()
            .filter_map(|id| state.points.get(id).cloned())
            .collect())
    }

    fn upsert(&self, _physical: &str, points: &[VectorPoint]) -> Result<(), Error> {
        let mut state = self.inner.lock().unwrap();
        if state.fail_upsert {
            return Err(vector_error("injected qdrant failure"));
        }
        for point in points {
            state.points.insert(
                point.record_id,
                VectorPointMetadata {
                    record_id: point.record_id,
                    record_sha256: point.record_sha256.clone(),
                    input_sha256: point.input_sha256.clone(),
                    model_sha256: self.config.embedding.model.sha256.clone(),
                },
            );
        }
        state.points_upserted += points.len();
        Ok(())
    }

    fn ensure_active(&self, physical: &str) -> Result<(), Error> {
        (physical == PHYSICAL)
            .then_some(())
            .ok_or_else(|| vector_error("active collection changed"))
    }

    fn mark_degraded(&self, physical: &str) -> Result<(), Error> {
        self.inner.lock().unwrap().degraded_marks += 1;
        super::super::state::write_degraded(&self.root, &self.config, physical)
    }

    fn mark_ready(&self, physical: &str) -> Result<(), Error> {
        self.inner.lock().unwrap().ready_marks += 1;
        super::super::state::stage_ready(&self.root, &self.config, physical)?.commit()
    }
}

struct FakeEmbedder {
    descriptor: EmbeddingDescriptor,
    append_during_embed: Option<PathBuf>,
}

impl Embedder for FakeEmbedder {
    fn descriptor(&self) -> &EmbeddingDescriptor {
        &self.descriptor
    }

    fn embed(&self, documents: &[EmbeddingDocument]) -> Result<Vec<Vec<f32>>, Error> {
        if let Some(root) = &self.append_during_embed {
            add(root, "concurrent");
        }
        Ok(documents
            .iter()
            .map(|_| vec![0.5; self.descriptor.dimensions as usize])
            .collect())
    }
}

#[test]
fn sync_upserts_delta_then_noop_loads_no_embedder() {
    let (root, config, index) = fixture("delta");
    let first = execute(&root, &config, &index, || Ok(embedder(&config, None))).unwrap();
    let second = execute(&root, &config, &index, || -> Result<FakeEmbedder, Error> {
        panic!("no-op sync loaded the embedder")
    })
    .unwrap();

    assert_eq!((first.embeddings, first.points_upserted), (1, 1));
    assert_eq!((second.embeddings, second.points_upserted), (0, 0));
    let counts = index.inner.lock().unwrap();
    assert_eq!(counts.points_upserted, 1);
    assert_eq!((counts.degraded_marks, counts.ready_marks), (1, 1));
    drop(counts);
    assert_eq!(state(&root).unwrap(), VectorState::Ready);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_upsert_keeps_ledger_and_state_degraded() {
    let (root, config, index) = fixture("failure");
    index.inner.lock().unwrap().fail_upsert = true;
    let before = corpus(&root).unwrap();

    assert!(execute(&root, &config, &index, || Ok(embedder(&config, None))).is_err());
    let after = corpus(&root).unwrap();
    assert_eq!((after.0.len(), after.1), (before.0.len(), before.1));
    assert_eq!(state(&root).unwrap(), VectorState::Degraded);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn concurrent_append_prevents_false_ready() {
    let (root, config, index) = fixture("concurrent");
    let result = execute(&root, &config, &index, || {
        Ok(embedder(&config, Some(root.clone())))
    });

    assert!(result.is_err());
    assert_eq!(corpus(&root).unwrap().0.len(), 2);
    assert_eq!(state(&root).unwrap(), VectorState::Degraded);
    fs::remove_dir_all(root).unwrap();
}

fn fixture(name: &str) -> (PathBuf, VectorConfig, FakeIndex) {
    let root = super::support::root(name);
    init::create(&root, "owner", "agent.memory").unwrap();
    schema::register(
        &root,
        TypeDefinition {
            type_name: "agent.lesson.v1".into(),
            uri: "equill://agent.lesson/v1".into(),
            owner: "owner".into(),
            payload_schema: json!({
                "type": "object",
                "properties": { "rule": { "type": "string" } },
                "required": ["rule"],
                "additionalProperties": false
            }),
            lifecycle: Default::default(),
        },
        "owner",
    )
    .unwrap();
    add(&root, "initial");
    super::support::write(&root, &super::support::config(&root));
    let config = super::super::config::load(&root).unwrap().unwrap();
    super::super::state::stage_ready(&root, &config, PHYSICAL)
        .unwrap()
        .commit()
        .unwrap();
    let index = FakeIndex {
        root: root.clone(),
        config: config.clone(),
        inner: Arc::default(),
    };
    (root, config, index)
}

fn embedder(config: &VectorConfig, append_during_embed: Option<PathBuf>) -> FakeEmbedder {
    FakeEmbedder {
        descriptor: EmbeddingDescriptor {
            model_id: config.embedding.model_id.clone(),
            model_sha256: config.embedding.model.sha256.clone(),
            tokenizer_sha256: config.embedding.tokenizer.sha256.clone(),
            dimensions: config.dimensions,
            distance: config.distance,
            input_schema: config.embedding.input_schema.clone(),
        },
        append_during_embed,
    }
}

fn add(root: &Path, rule: &str) {
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
    .unwrap();
}
