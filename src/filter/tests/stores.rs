//! Stores a filter test can run against.
use crate::schema::TypeDefinition;
use serde_json::json;
use uuid::Uuid;

/// Three synthetic records across two types, so a scope can be shown to be
/// smaller than the store.
pub fn scoped_store() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("equill-scope-{}", Uuid::now_v7()));
    crate::command::init::create(&root, "owner", "agent.memory").expect("initialize");
    for type_name in ["agent.lesson.v1", "agent.lesson.v1", "agent.other.v1"] {
        register_type(&root, type_name);
        crate::record::append(
            &root,
            crate::record::RecordDraft {
                namespace: "agent.memory".into(),
                type_name: type_name.into(),
                observed_at: "2026-01-01T00:00:00Z".into(),
                valid_at: None,
                payload: json!({ "rule": "synthetic" }),
                evidence: Vec::new(),
                tags: Vec::new(),
                supersedes: None,
            },
            "owner",
        )
        .expect("append");
    }
    root
}

fn register_type(root: &std::path::Path, type_name: &str) {
    if crate::schema::load(root, type_name).is_ok() {
        return;
    }
    let (base, version) = type_name.rsplit_once('.').expect("versioned type");
    crate::schema::register(
        root,
        TypeDefinition {
            type_name: type_name.into(),
            uri: format!("equill://{base}/{version}"),
            owner: "owner".into(),
            payload_schema: json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": { "rule": { "type": "string" } },
                "required": ["rule"],
                "additionalProperties": false
            }),
            lifecycle: Default::default(),
        },
        "owner",
    )
    .expect("register schema");
}
