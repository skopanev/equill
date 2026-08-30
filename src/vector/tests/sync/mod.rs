mod concurrency;
mod endpoint_consistency;
mod freshness;

use crate::command::init;
use crate::kernel::error::Error;
use crate::record::{RecordDraft, append};
use crate::schema::{self, TypeDefinition};
use crate::vector::model::{
    EmbeddingDescriptor, EmbeddingDocument, VectorPoint, VectorPointMetadata, vector_error,
};
use crate::vector::operator::{SyncIndex, execute, execute_with_progress};
use crate::vector::{Embedder, VectorConfig, VectorState, corpus, state};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

const PHYSICAL: &str = "equill_sync_active";

#[derive(Clone)]
pub(super) struct FakeIndex {
    root: PathBuf,
    config: VectorConfig,
    pub(super) inner: Arc<Mutex<FakeState>>,
}

#[derive(Default)]
pub(super) struct FakeState {
    pub(super) points: HashMap<Uuid, VectorPointMetadata>,
    pub(super) points_upserted: usize,
    pub(super) ready_marks: usize,
    pub(super) checkpoint: Option<(usize, String)>,
    pub(super) fail_upsert: bool,
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

    fn mark_indexed(&self, physical: &str, records: usize, digest: &str) -> Result<(), Error> {
        let mut inner = self.inner.lock().unwrap();
        inner.ready_marks += 1;
        inner.checkpoint = Some((records, digest.to_owned()));
        drop(inner);
        crate::vector::state::stage_ready(
            &self.root,
            &self.config,
            physical,
            Some((records, digest)),
        )?
        .commit()
    }
}

pub(super) struct FakeEmbedder {
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
    let mut first_events = Vec::new();
    let first = {
        let mut sink = |event| first_events.push(event);
        execute_with_progress(
            &root,
            &config,
            &index,
            || Ok(embedder(&config, None)),
            Some(&mut sink),
        )
        .unwrap()
    };
    let mut noop_events = Vec::new();
    let second = {
        let mut sink = |event| noop_events.push(event);
        execute_with_progress(
            &root,
            &config,
            &index,
            || -> Result<FakeEmbedder, Error> { panic!("no-op sync loaded the embedder") },
            Some(&mut sink),
        )
        .unwrap()
    };

    assert_eq!((first.embeddings, first.points_upserted), (1, 1));
    assert_eq!((second.embeddings, second.points_upserted), (0, 0));
    assert_eq!(
        first_events,
        super::support::sync_events(PHYSICAL, &first.corpus_sha256, 1)
    );
    assert_eq!(
        noop_events,
        super::support::sync_events(PHYSICAL, &second.corpus_sha256, 0)
    );
    let counts = index.inner.lock().unwrap();
    assert_eq!(counts.points_upserted, 1);
    // The checkpoint is written on every pass, including the no-op one: that is
    // how a store nobody wrote to becomes current rather than staying lagging.
    assert_eq!(counts.ready_marks, 2);
    drop(counts);
    assert_eq!(state(&root).unwrap(), VectorState::Ready);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_failed_pass_keeps_the_last_searchable_checkpoint() {
    let (root, config, index) = fixture("failure");
    index.inner.lock().unwrap().fail_upsert = true;
    let before = corpus(&root).unwrap();

    assert!(execute(&root, &config, &index, || Ok(embedder(&config, None))).is_err());
    let after = corpus(&root).unwrap();
    assert_eq!((after.0.len(), after.1), (before.0.len(), before.1));
    // The previous checkpoint survives a failed pass: losing a working index
    // to protect it from being slightly behind is the worse outcome.
    assert_eq!(state(&root).unwrap(), VectorState::Ready);
    fs::remove_dir_all(root).unwrap();
}

pub(super) fn fixture(name: &str) -> (PathBuf, VectorConfig, FakeIndex) {
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
    crate::vector::state::stage_ready(&root, &config, PHYSICAL, None)
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

pub(super) fn embedder(
    config: &VectorConfig,
    append_during_embed: Option<PathBuf>,
) -> FakeEmbedder {
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

pub(super) fn add(root: &Path, rule: &str) {
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
