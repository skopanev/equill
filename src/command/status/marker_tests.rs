//! Whether the two halves of a status report describe the same index.
//!
//! The store block and the component block answer the same question from the
//! same marker, and a reader has no way to choose between them when they
//! disagree. These read a whole report and compare the halves rather than
//! calling the assessment directly, because agreement is the property at stake.
use super::report;
use crate::record::{RecordDraft, append, read_all};
use crate::vector::operator::execute;
use crate::vector::tests::sync::{embedder, fixture};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

fn synced(name: &str) -> PathBuf {
    let (root, config, index) = fixture(name);
    execute(&root, &config, &index, || Ok(embedder(&config, None))).expect("sync");
    root
}

fn json(root: &Path) -> Value {
    serde_json::to_value(report(Some(root)).expect("status")).expect("serialize status")
}

fn vector_component(value: &Value) -> Value {
    value["components"]
        .as_array()
        .expect("components")
        .iter()
        .find(|component| component["id"] == "vector.qdrant")
        .expect("vector component")
        .clone()
}

/// Replaces the one record the fixture wrote. The corpus keeps its length while
/// its content moves on, which is the case a count subtraction reads as zero.
fn replace(root: &Path) {
    let target = read_all(root).expect("records")[0].id;
    append(
        root,
        RecordDraft {
            namespace: "agent.memory".into(),
            type_name: "agent.lesson.v1".into(),
            observed_at: "2026-01-01T00:00:00Z".into(),
            valid_at: None,
            payload: json!({ "rule": "a corrected rule" }),
            evidence: Vec::new(),
            tags: Vec::new(),
            supersedes: Some(target),
        },
        "owner",
    )
    .expect("supersede");
}

/// A replacement leaves the corpus the same size, so the component's
/// `len - indexed` came out as zero next to a store block saying the size of the
/// backlog is unknown. One document, two answers, and nothing to choose between.
#[test]
fn a_replacement_is_unknown_on_both_halves_rather_than_zero_on_one() {
    let root = synced("status-replacement");
    replace(&root);
    let value = json(&root);
    let component = vector_component(&value);

    assert_eq!(value["store"]["vector_pending"]["kind"], "unknown");
    assert_eq!(
        component["vector_pending_records"],
        Value::Null,
        "the component put a number on a backlog the store block could not size"
    );
    assert_eq!(component["vector_freshness"], "lagging");
    fs::remove_dir_all(root).expect("remove test store");
}

/// A marker claiming a revision the store never published is not this store's
/// position, whatever digest it carries. Freshness had always refused it; the
/// store block had not, so the same marker read as caught up and as unknown in
/// one report — and the caught-up half was the one the human line printed.
#[test]
fn a_checkpoint_ahead_of_every_published_revision_is_not_up_to_date() {
    let root = synced("status-ahead");
    let path = root.join("projections/qdrant/state.json");
    let mut marker: Value =
        serde_json::from_slice(&fs::read(&path).expect("marker")).expect("json");
    marker
        .as_object_mut()
        .expect("marker object")
        .insert("indexed_revision".into(), json!(1_000_000));
    fs::write(&path, serde_json::to_vec(&marker).expect("bytes")).expect("write marker");

    let value = json(&root);
    let component = vector_component(&value);
    let text = crate::command::output::status(&report(Some(&root)).expect("status"));

    assert_eq!(value["store"]["vector_pending"]["kind"], "unknown");
    assert_eq!(value["store"]["vector_checkpoint_records"], Value::Null);
    assert_eq!(component["vector_freshness"], "unknown");
    assert_eq!(component["vector_pending_records"], Value::Null);
    assert!(
        !text.contains("up to date"),
        "the human line called a store with an unreadable checkpoint current:\n{text}"
    );
    fs::remove_dir_all(root).expect("remove test store");
}

/// How much there is to embed is a property of the ledger. Dropping it when no
/// provider is configured hides the number from the only reader who has to
/// decide whether to configure one.
#[test]
fn a_store_without_a_provider_still_counts_what_it_would_embed() {
    let root = crate::vector::tests::support::root("status-unconfigured");
    crate::command::init::create(&root, "owner", "agent.memory").expect("initialize");
    let value = json(&root);
    let component = vector_component(&value);

    assert_eq!(value["store"]["vector_eligible_records"], 0);
    assert_eq!(value["store"]["vector_processing"], "not_tracked");
    assert_eq!(value["store"]["vector_checkpoint_records"], Value::Null);
    assert_eq!(
        value["store"]["vector_pending"]["reason"],
        "no vector projection is configured"
    );
    assert_eq!(component["vector_state"], "disabled");
    fs::remove_dir_all(root).expect("remove test store");
}
