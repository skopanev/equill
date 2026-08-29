use super::super::{assemble, inline_request, register_profile, register_selector};
use crate::command::init;
use crate::filter::Filter;
use crate::record::{self, RecordDraft};
use crate::schema::{self, LifecycleMode, LifecyclePolicy, TypeDefinition};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Equill does not own the domain's vocabulary. A field like `role` is the
/// client's, its legal values are the client's, and the engine must not need to
/// know them — which is what this proves: an unconstrained field is filtered,
/// matched as a coordinate, and a record that leaves it null still applies
/// everywhere, exactly as the selector's wildcard mode promises.
#[test]
fn an_open_vocabulary_field_needs_no_engine_knowledge() {
    let root = store("vocabulary");
    register_type(&root, "agent.lesson.v1", &[]);
    register_context(&root, &["agent.lesson.v1"]);
    let universal = add(&root, "agent.lesson.v1", "Applies to everyone", None, None);
    let scoped = add(
        &root,
        "agent.lesson.v1",
        "Applies to reviewers",
        Some(json!(["reviewer", "auditor"])),
        None,
    );

    let asked = bundle(&root, "Applies", Some("role=reviewer"));
    let unknown_value = bundle(&root, "Applies", Some("role=nobody-uses-this"));

    // A value the engine has never seen matches by membership; a record that
    // left the field null is universal rather than excluded.
    assert!(asked.contains(&scoped) && asked.contains(&universal));
    assert!(unknown_value.contains(&universal) && !unknown_value.contains(&scoped));
    fs::remove_dir_all(root).expect("cleanup");
}

/// The answer to "a type is immutable and understanding is not" is a second
/// version that names the first. This proves the path end to end: a v2 record
/// supersedes its v1 predecessor across the version boundary, readers see only
/// the current claim, and the chain is still there when asked for.
#[test]
fn a_second_version_supersedes_the_first_across_the_boundary() {
    let root = store("migration");
    register_type(&root, "agent.lesson.v1", &[]);
    register_type(&root, "agent.lesson.v2", &["agent.lesson.v1"]);
    register_context(&root, &["agent.lesson.v1", "agent.lesson.v2"]);
    let old = add(&root, "agent.lesson.v1", "First understanding", None, None);
    let new = add(
        &root,
        "agent.lesson.v2",
        "Corrected understanding",
        None,
        Some(old),
    );

    let current = bundle(&root, "understanding", None);
    let chain = full_chain(&root, "understanding");

    assert!(current.contains(&new));
    assert!(!current.contains(&old), "a superseded claim is not current");
    assert!(chain.contains(&old) && chain.contains(&new));
    fs::remove_dir_all(root).expect("cleanup");
}

fn bundle(root: &Path, query: &str, filter: Option<&str>) -> Vec<Uuid> {
    let filter = Filter::parse(
        &filter
            .map(|item| vec![item.to_string()])
            .unwrap_or_default(),
        false,
    )
    .expect("filter");
    let request = inline_request(
        Some(query.into()),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        None,
        false,
    )
    .expect("request");
    assemble(root, "worker.v1", request, "owner", &filter)
        .expect("context")
        .selected_record_ids
}

fn full_chain(root: &Path, query: &str) -> Vec<Uuid> {
    let request = inline_request(
        Some(query.into()),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        None,
        true,
    )
    .expect("request");
    assemble(root, "worker.v1", request, "owner", &Filter::default())
        .expect("context")
        .selected_record_ids
}

fn store(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("equill-vocab-{name}-{}", Uuid::now_v7()));
    init::create(&root, "owner", "agent.memory").expect("initialize");
    root
}

/// The payload leaves `role` unconstrained on purpose: the engine stores and
/// retrieves it without ever being told what a role is.
fn register_type(root: &Path, type_name: &str, predecessors: &[&str]) {
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
                "properties": {
                    "rule": { "type": "string" },
                    "role": { "type": ["array", "string", "null"], "items": { "type": "string" } }
                },
                "required": ["rule"],
                "additionalProperties": false
            }),
            lifecycle: LifecyclePolicy {
                mode: LifecycleMode::Dag,
                key_pointer: None,
                allowed_predecessor_types: predecessors.iter().map(|item| (*item).into()).collect(),
            },
        },
        "owner",
    )
    .expect("register schema");
}

fn register_selector_for(root: &Path, type_name: &str) {
    let path = root.join(format!("selector-{type_name}.json"));
    fs::write(
        &path,
        serde_json::to_vec(&json!({
            "id": format!("{type_name}.selector"),
            "version": "1",
            "type": type_name,
            "strategies": ["exact", "tag"],
            "coordinate_pointers": { "role": "/role" },
            "coordinate_modes": { "role": "set_or_wildcard" }
        }))
        .expect("selector json"),
    )
    .expect("selector file");
    register_selector(root, &path, "owner").expect("selector");
}

fn register_context(root: &Path, types: &[&str]) {
    for type_name in types {
        register_selector_for(root, type_name);
    }
    let profile = root.join("profile.json");
    fs::write(
        &profile,
        serde_json::to_vec(&json!({
            "id": "worker.v1",
            "version": "1",
            "actors": [],
            "grants": [{ "namespace": "agent.memory", "types": types }],
            "selectors": types
                .iter()
                .map(|type_name| format!("{type_name}.selector"))
                .collect::<Vec<_>>()
        }))
        .expect("profile json"),
    )
    .expect("profile file");
    register_profile(root, &profile, "owner").expect("profile");
}

fn add(
    root: &Path,
    type_name: &str,
    rule: &str,
    role: Option<serde_json::Value>,
    supersedes: Option<Uuid>,
) -> Uuid {
    let mut payload = json!({ "rule": rule });
    if let Some(role) = role {
        payload["role"] = role;
    }
    record::append(
        root,
        RecordDraft {
            namespace: "agent.memory".into(),
            type_name: type_name.into(),
            observed_at: "2026-01-01T00:00:00Z".into(),
            valid_at: None,
            payload,
            evidence: Vec::new(),
            tags: Vec::new(),
            supersedes,
        },
        "owner",
    )
    .expect("append")
    .id
}
