use super::{embedder, fixture};
use crate::record::{RecordDraft, append, revoke};
use crate::vector::corpus;
use crate::vector::operator::execute;
use serde_json::json;
use std::fs;
use std::path::Path;
use uuid::Uuid;

#[test]
fn sync_removes_superseded_and_revoked_vectors() {
    let (root, config, index) = fixture("lifecycle-prune");
    let first = corpus(&root).unwrap().0[0].0.id;
    execute(&root, &config, &index, || Ok(embedder(&config, None))).unwrap();
    let replacement = replace(&root, first, "replacement");
    execute(&root, &config, &index, || Ok(embedder(&config, None))).unwrap();
    assert_eq!(index.inner.lock().unwrap().points.len(), 1);
    assert!(
        index
            .inner
            .lock()
            .unwrap()
            .points
            .contains_key(&replacement)
    );

    revoke(&root, replacement, Some("obsolete"), "owner").unwrap();
    execute(&root, &config, &index, || Ok(embedder(&config, None))).unwrap();
    assert!(index.inner.lock().unwrap().points.is_empty());
    fs::remove_dir_all(root).unwrap();
}

fn replace(root: &Path, target: Uuid, rule: &str) -> Uuid {
    append(
        root,
        RecordDraft {
            namespace: "agent.memory".into(),
            type_name: "agent.lesson.v1".into(),
            observed_at: "2026-01-02T00:00:00Z".into(),
            valid_at: None,
            payload: json!({ "rule": rule }),
            evidence: Vec::new(),
            tags: Vec::new(),
            supersedes: Some(target),
        },
        "owner",
    )
    .unwrap()
    .id
}
