use super::super::model::ExclusionReason;
use super::super::{ContextRequest, assemble, register_profile, register_selector};
use crate::command::init;
use crate::filter::Filter;
use crate::record::{self, RecordDraft};
use crate::schema::{self, LifecycleMode, LifecyclePolicy, TypeDefinition};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[test]
fn hidden_cross_type_successor_does_not_mask_visible_predecessor() {
    let root = store();
    register_types(&root);
    let predecessor = append(&root, "agent.lesson.v1", "Visible predecessor", None);
    let successor = append(
        &root,
        "agent.lesson.v2",
        "Hidden successor",
        Some(predecessor),
    );
    register_context(&root);

    for (profile, reason) in [
        ("worker.v1-only", ExclusionReason::Unauthorized),
        ("worker.no-v2-selector", ExclusionReason::SelectorMismatch),
    ] {
        let bundle = assemble(
            &root,
            profile,
            request("visible predecessor"),
            "owner",
            &Filter::default(),
        )
        .expect("context");
        assert_eq!(bundle.selected_record_ids, vec![predecessor]);
        assert!(
            bundle
                .receipt
                .excluded
                .iter()
                .any(|item| { item.id == successor && item.reason == reason })
        );
        assert!(
            !bundle.receipt.excluded.iter().any(|item| {
                item.id == predecessor && item.reason == ExclusionReason::Superseded
            })
        );
    }
    let visible = assemble(
        &root,
        "worker.all",
        request("hidden successor"),
        "owner",
        &Filter::default(),
    )
    .expect("fully visible successor");
    assert_eq!(visible.selected_record_ids, vec![successor]);
    assert!(
        visible
            .receipt
            .excluded
            .iter()
            .any(|item| { item.id == predecessor && item.reason == ExclusionReason::Superseded })
    );
    fs::remove_dir_all(root).expect("cleanup");
}

fn store() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "equill-context-visibility-{}-{}",
        std::process::id(),
        Uuid::now_v7()
    ));
    init::create(&root, "owner", "agent.memory").expect("initialize");
    root
}

fn register_types(root: &Path) {
    schema::register(root, definition("agent.lesson.v1", &[]), "owner").expect("v1 schema");
    schema::register(
        root,
        definition("agent.lesson.v2", &["agent.lesson.v1"]),
        "owner",
    )
    .expect("v2 schema");
}

fn definition(type_name: &str, predecessors: &[&str]) -> TypeDefinition {
    let (base, version) = type_name.rsplit_once('.').expect("versioned type");
    TypeDefinition {
        type_name: type_name.into(),
        uri: format!("equill://{base}/{version}"),
        owner: "owner".into(),
        payload_schema: json!({
            "type": "object",
            "properties": { "rule": { "type": "string" } },
            "required": ["rule"],
            "additionalProperties": false
        }),
        lifecycle: LifecyclePolicy {
            mode: LifecycleMode::Dag,
            key_pointer: None,
            allowed_predecessor_types: predecessors.iter().map(|item| (*item).into()).collect(),
        },
    }
}

fn append(root: &Path, type_name: &str, rule: &str, supersedes: Option<Uuid>) -> Uuid {
    record::append(
        root,
        RecordDraft {
            namespace: "agent.memory".into(),
            type_name: type_name.into(),
            observed_at: "2026-01-01T00:00:00Z".into(),
            valid_at: None,
            payload: json!({ "rule": rule }),
            evidence: Vec::new(),
            tags: Vec::new(),
            supersedes,
        },
        "owner",
    )
    .expect("append")
    .id
}

fn register_context(root: &Path) {
    for version in ["v1", "v2"] {
        let selector = root.join(format!("selector-{version}.json"));
        fs::write(
            &selector,
            serde_json::to_vec(&json!({
                "id": format!("lesson.{version}.selector"),
                "version": "1",
                "type": format!("agent.lesson.{version}"),
                "strategies": ["exact"]
            }))
            .expect("selector json"),
        )
        .expect("selector file");
        register_selector(root, &selector, "owner").expect("selector");
    }
    for (id, types, selectors) in [
        (
            "worker.v1-only",
            vec!["agent.lesson.v1"],
            vec!["lesson.v1.selector"],
        ),
        (
            "worker.no-v2-selector",
            vec!["agent.lesson.v1", "agent.lesson.v2"],
            vec!["lesson.v1.selector"],
        ),
        (
            "worker.all",
            vec!["agent.lesson.v1", "agent.lesson.v2"],
            vec!["lesson.v1.selector", "lesson.v2.selector"],
        ),
    ] {
        let profile = root.join(format!("{id}.json"));
        fs::write(
            &profile,
            serde_json::to_vec(&json!({
                "id": id,
                "version": "1",
                "actors": [],
                "grants": [{ "namespace": "agent.memory", "types": types }],
                "selectors": selectors
            }))
            .expect("profile json"),
        )
        .expect("profile file");
        register_profile(root, &profile, "owner").expect("profile");
    }
}

fn request(query: &str) -> ContextRequest {
    ContextRequest {
        at: "2026-01-05T00:00:00Z".into(),
        query: query.into(),
        tags: Vec::new(),
        kinds: Vec::new(),
        coordinates: BTreeMap::new(),
        include_superseded: false,
    }
}
