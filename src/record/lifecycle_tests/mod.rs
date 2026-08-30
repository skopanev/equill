mod chains;
mod graph;
mod integrity;
mod modes;

use super::{RecordDraft, StoredRecord, append, lifecycle, read_all};
use crate::command::init;
use crate::schema::{self, LifecycleMode, LifecyclePolicy, TypeDefinition};
use serde_json::json;
use std::path::{Path, PathBuf};
use uuid::Uuid;

fn store(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "equill-lifecycle-{name}-{}-{}",
        std::process::id(),
        Uuid::now_v7()
    ));
    init::create(&root, "owner", "agent.memory").expect("initialize");
    root
}

fn register(root: &Path, type_name: &str, lifecycle: LifecyclePolicy) {
    let (base, version) = type_name.rsplit_once('.').expect("versioned type");
    schema::register(
        root,
        TypeDefinition {
            type_name: type_name.into(),
            uri: format!("equill://{base}/{version}"),
            owner: "owner".into(),
            payload_schema: json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": { "key": { "type": "string" }, "rule": { "type": "string" } },
                "required": ["rule"],
                "additionalProperties": false
            }),
            lifecycle,
        },
        "owner",
    )
    .expect("register schema");
}

fn linear(predecessors: &[&str]) -> LifecyclePolicy {
    LifecyclePolicy {
        mode: LifecycleMode::Linear,
        key_pointer: Some("/key".into()),
        allowed_predecessor_types: predecessors.iter().map(|item| (*item).into()).collect(),
    }
}

fn dag(predecessors: &[&str]) -> LifecyclePolicy {
    LifecyclePolicy {
        mode: LifecycleMode::Dag,
        key_pointer: None,
        allowed_predecessor_types: predecessors.iter().map(|item| (*item).into()).collect(),
    }
}

fn draft(type_name: &str, rule: &str, supersedes: Option<Uuid>) -> RecordDraft {
    RecordDraft {
        namespace: "agent.memory".into(),
        type_name: type_name.into(),
        observed_at: "2026-01-01T00:00:00Z".into(),
        valid_at: None,
        payload: json!({ "key": "shared", "rule": rule }),
        evidence: Vec::new(),
        tags: Vec::new(),
        supersedes,
    }
}

fn stored(namespace: &str, type_name: &str, supersedes: Option<Uuid>) -> StoredRecord {
    StoredRecord {
        id: Uuid::now_v7(),
        namespace: namespace.into(),
        type_name: type_name.into(),
        actor: "owner".into(),
        recorded_at: "2026-01-01T00:00:00Z".into(),
        observed_at: "2026-01-01T00:00:00Z".into(),
        valid_at: "2026-01-01T00:00:00Z".into(),
        payload: json!({ "key": "shared", "rule": "synthetic" }),
        evidence: Vec::new(),
        tags: Vec::new(),
        supersedes,
    }
}
