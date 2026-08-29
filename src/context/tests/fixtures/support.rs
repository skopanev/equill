//! The store a context test opens and the request it sends.
use super::super::super::ContextRequest;
use crate::command::init;
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

pub fn request(query: &str) -> ContextRequest {
    ContextRequest {
        at: "2026-01-05T00:00:00Z".into(),
        query: query.into(),
        tags: vec![],
        kinds: vec![],
        coordinates: BTreeMap::new(),
        include_superseded: false,
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
