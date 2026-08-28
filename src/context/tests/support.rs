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
    registry_with_modes(
        root,
        total,
        required_cap,
        strategies,
        grant_namespace,
        json!({}),
    );
}

pub fn registry_with_modes(
    root: &Path,
    total: usize,
    required_cap: usize,
    strategies: &[&str],
    grant_namespace: &str,
    coordinate_modes: serde_json::Value,
) {
    registry_with_options(
        root,
        total,
        required_cap,
        strategies,
        grant_namespace,
        coordinate_modes,
        None,
    );
}

pub fn registry_with_rank(
    root: &Path,
    total: usize,
    required_cap: usize,
    strategies: &[&str],
    rank_pointer: &str,
) {
    registry_with_options(
        root,
        total,
        required_cap,
        strategies,
        "agent.memory",
        json!({}),
        Some(rank_pointer),
    );
}

fn registry_with_options(
    root: &Path,
    total: usize,
    required_cap: usize,
    strategies: &[&str],
    grant_namespace: &str,
    coordinate_modes: serde_json::Value,
    rank_pointer: Option<&str>,
) {
    let core_cap = total.saturating_sub(20);
    let relevant_floor = (total / 4).min(500);
    let selector = root.join("selector.json");
    let mut definition = json!({
        "id": "agent.lesson.inject.v1",
        "version": "1",
        "type": "agent.lesson.v1",
        "strategies": strategies,
        "required_tags": ["must"],
        "core_tags": ["core"],
        "coordinate_pointers": { "scope": "/scope" },
        "coordinate_modes": coordinate_modes
    });
    if let Some(pointer) = rank_pointer {
        definition["rank_pointer"] = json!(pointer);
    }
    fs::write(
        &selector,
        serde_json::to_vec(&definition).expect("selector json"),
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

/// Profile with no budget block at all — every bound absent. Nothing is capped
/// and the required tier can never overflow.
pub fn registry_unbounded(root: &Path, strategies: &[&str], grant_namespace: &str) {
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
            "coordinate_pointers": { "scope": "/scope" },
            "coordinate_modes": {}
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
            "selectors": ["agent.lesson.inject.v1"]
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
    append_coordinate(
        root,
        rule,
        tags,
        supersedes,
        valid_at,
        scope.map(|value| json!(value)),
    )
}

pub fn append_coordinate(
    root: &Path,
    rule: &str,
    tags: &[&str],
    supersedes: Option<uuid::Uuid>,
    valid_at: &str,
    scope: Option<serde_json::Value>,
) -> uuid::Uuid {
    let mut payload = json!({ "rule": rule });
    if let Some(scope) = scope {
        payload["scope"] = scope;
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

pub fn append_ranked(root: &Path, rule: &str, confidence: f64, observed_at: &str) -> uuid::Uuid {
    record::append(
        root,
        RecordDraft {
            namespace: "agent.memory".into(),
            type_name: "agent.lesson.v1".into(),
            observed_at: observed_at.into(),
            valid_at: Some("2026-01-01T00:00:00Z".into()),
            payload: json!({ "rule": rule, "confidence": confidence }),
            evidence: vec![],
            tags: vec![],
            supersedes: None,
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
                "confidence": { "type": "number" },
                "scope": {
                    "type": ["string", "array", "null"],
                    "items": { "type": "string" }
                }
            },
            "x-equill-envelope": { "namespace": "agent.memory", "type": "agent.lesson.v1" }
        }))
        .expect("schema json"),
    )
    .expect("schema file");
    schema::register_file(root, &path, "test-owner").expect("register schema");
}
