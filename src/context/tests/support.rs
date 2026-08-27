use super::super::{ContextRequest, register_profile, register_selector};
use crate::command::init;
use crate::record::{self, RecordDraft};
use crate::schema;
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);

pub fn store(name: &str) -> PathBuf {
    let suffix = NEXT.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "equill-context-{name}-{}-{suffix}",
        std::process::id()
    ));
    init::create(&root, "test-owner", "agent.memory").expect("initialize");
    register_type(&root);
    root
}

pub fn registry(
    root: &Path,
    total: usize,
    required_cap: usize,
    strategies: &[&str],
    grant_namespace: &str,
) {
    let core_cap = total.saturating_sub(20);
    let relevant_floor = (total / 4).min(500);
    let selector = root.join("selector.json");
    fs::write(
        &selector,
        serde_json::to_vec(&json!({
            "id": "agent.lesson.inject.v1",
            "version": "1",
            "type": "agent.lesson.v1",
            "strategies": strategies,
            "required_tags": ["must"],
            "core_tags": ["core"],
            "coordinate_pointers": { "scope": "/scope" }
        }))
        .expect("selector json"),
    )
    .expect("selector file");
    register_selector(root, &selector, "test-owner").expect("register selector");
    let profile = root.join("profile.json");
    fs::write(
        &profile,
        serde_json::to_vec(&json!({
            "id": "worker.v1",
            "version": "1",
            "actors": [],
            "grants": [{ "namespace": grant_namespace, "types": ["agent.lesson.v1"] }],
            "selectors": ["agent.lesson.inject.v1"],
            "budget": {
                "total": total,
                "required_cap": required_cap,
                "core_cap": core_cap,
                "relevant_floor": relevant_floor,
                "receipt_reserve": 20
            }
        }))
        .expect("profile json"),
    )
    .expect("profile file");
    register_profile(root, &profile, "test-owner").expect("register profile");
}

pub fn append(
    root: &Path,
    rule: &str,
    tags: &[&str],
    supersedes: Option<uuid::Uuid>,
    valid_at: &str,
) -> uuid::Uuid {
    append_scoped(root, rule, tags, supersedes, valid_at, None)
}

pub fn append_scoped(
    root: &Path,
    rule: &str,
    tags: &[&str],
    supersedes: Option<uuid::Uuid>,
    valid_at: &str,
    scope: Option<&str>,
) -> uuid::Uuid {
    let mut payload = json!({ "rule": rule });
    if let Some(scope) = scope {
        payload["scope"] = json!(scope);
    }
    record::append(
        root,
        RecordDraft {
            namespace: "agent.memory".into(),
            type_name: "agent.lesson.v1".into(),
            observed_at: "2026-01-01T00:00:00Z".into(),
            valid_at: Some(valid_at.into()),
            payload,
            evidence: vec![],
            tags: tags.iter().map(|tag| (*tag).into()).collect(),
            supersedes,
        },
        "test-owner",
    )
    .expect("append")
    .id
}

pub fn request(query: &str) -> ContextRequest {
    ContextRequest {
        at: "2026-01-05T00:00:00Z".into(),
        query: query.into(),
        tags: vec![],
        kinds: vec![],
        coordinates: BTreeMap::new(),
    }
}

fn register_type(root: &Path) {
    let path = root.join("lesson.schema.json");
    fs::write(
        &path,
        serde_json::to_vec(&json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": "equill://agent.lesson/v1",
            "type": "object",
            "required": ["rule"],
            "additionalProperties": false,
            "properties": {
                "rule": { "type": "string" },
                "scope": { "type": "string" }
            },
            "x-equill-envelope": { "namespace": "agent.memory", "type": "agent.lesson.v1" }
        }))
        .expect("schema json"),
    )
    .expect("schema file");
    schema::register_file(root, &path, "test-owner").expect("register schema");
}
