//! Record fixtures for context tests: what the store actually holds.
use crate::record::{self, RecordDraft};
use serde_json::json;
use std::path::Path;

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
