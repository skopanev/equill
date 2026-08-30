use crate::governance::tests::{interrupt, journal_exists, store};
use crate::governance::{grant, metadata, recover, revoke_grant, transfer};
use crate::kernel::store as config;
use serde_json::json;
use std::path::Path;

/// A store written by a newer Equill may carry top-level keys this build has
/// never heard of. Reading them is not enough — anything that rewrites the
/// metadata has to write them back, or an ordinary grant silently deletes state
/// the newer build depends on.
#[test]
fn unknown_top_level_fields_survive_every_governance_mutation() {
    let root = store();
    seed(&root);

    grant(
        &root,
        "worker",
        "agent.memory",
        &["agent.lesson.v1".into()],
        None,
        "founder",
    )
    .expect("grant");
    assert_eq!(unknown(&root), expected(), "a grant must not erase it");

    revoke_grant(&root, "worker", None, "founder").expect("revoke");
    assert_eq!(unknown(&root), expected(), "nor a revoke");

    transfer(&root, "successor", None, "founder").expect("transfer");
    assert_eq!(unknown(&root), expected(), "nor a handover");
    assert_eq!(config::load(&root).expect("load").root_owner, "successor");
}

/// Recovery writes metadata too, from bytes it holds rather than bytes it
/// derives — so the unknown fields have to be in those bytes as well.
#[test]
fn unknown_fields_survive_an_interrupted_transaction_and_its_recovery() {
    let root = store();
    seed(&root);
    interrupt(&root, "successor");

    let outcome = recover(&root).expect("recovery");

    assert!(outcome.is_some_and(|text| text.starts_with("completed")));
    assert_eq!(config::load(&root).expect("load").root_owner, "successor");
    assert_eq!(unknown(&root), expected(), "recovery carried them through");
    assert!(!journal_exists(&root));
}

/// The round trip is byte-deterministic: repeating a no-op change rewrites
/// nothing, which is what lets a digest be compared at all.
#[test]
fn a_no_op_leaves_the_metadata_byte_identical() {
    let root = store();
    seed(&root);
    let types = ["agent.lesson.v1".to_string()];
    grant(&root, "worker", "agent.memory", &types, None, "founder").expect("first");
    let after_first = std::fs::read(root.join("store.json")).expect("metadata");
    let digest = metadata::digest(&root).expect("digest");

    let again = grant(&root, "worker", "agent.memory", &types, None, "founder").expect("second");

    assert!(!again.changed);
    assert_eq!(
        std::fs::read(root.join("store.json")).expect("metadata"),
        after_first
    );
    assert_eq!(again.store_sha256, digest);
}

fn seed(root: &Path) {
    let path = root.join("store.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("metadata")).expect("json");
    let object = value.as_object_mut().expect("object");
    object.insert("future_field".into(), expected());
    std::fs::write(&path, serde_json::to_vec_pretty(&value).expect("json")).expect("seed");
}

fn expected() -> serde_json::Value {
    json!({ "nested": { "list": [1, 2, { "deep": true }], "text": "keep" }, "version": 7 })
}

fn unknown(root: &Path) -> serde_json::Value {
    config::load(root)
        .expect("load")
        .extra
        .get("future_field")
        .cloned()
        .unwrap_or(serde_json::Value::Null)
}
