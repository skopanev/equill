//! A store that names which types it embeds.
//!
//! The saving is easy; the trap is what happens at the moment the list changes.
//! The gate that decides whether a pass runs answers from two small marker
//! files and never scans the ledger, so a corpus that quietly became smaller
//! looks exactly like a corpus that did not change — and the vectors of a type
//! the store just stopped embedding go on answering searches that nothing in
//! the ledger accounts for.
use super::{FakeIndex, embedder, fixture};
use crate::record::{RecordDraft, append};
use crate::schema::{self, TypeDefinition};
use crate::vector::VectorConfig;
use crate::vector::catchup::drain::outstanding_for_tests;
use crate::vector::operator::execute;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub(super) const NOTE: &str = "agent.note.v1";

/// A store holding one lesson and one note, both embedded, both indexed.
pub(super) fn two_types(name: &str) -> (PathBuf, VectorConfig, FakeIndex, Uuid, Uuid) {
    let (root, config, index) = fixture(name);
    register_note(&root);
    let lesson = crate::vector::corpus(&root).expect("corpus").0[0].0.id;
    let note = add_note(&root, "a passing thought");
    execute(&root, &config, &index, || Ok(embedder(&config, None))).expect("first sync");
    assert_eq!(
        index.inner.lock().unwrap().points.len(),
        2,
        "the premise: both types are indexed before the filter exists"
    );
    (root, config, index, lesson, note)
}

/// Rewrites the descriptor through the ordinary configure path, which is the
/// only one that can be trusted to do whatever configure does.
///
/// The stored descriptor is edited rather than rebuilt: a fresh one would carry
/// a new `store_id`, and the marker would then describe another store — which
/// leaves work outstanding for a reason that has nothing to do with the filter.
pub(super) fn set_embed_types(root: &Path, types: &[&str]) {
    let mut descriptor = stored(root);
    descriptor["embed_types"] = json!(types);
    let file = root.join("candidate.json");
    fs::write(&file, serde_json::to_vec(&descriptor).expect("json")).expect("candidate");
    crate::vector::configure(root, &file, "owner").expect("configure");
}

/// Narrowing the filter has to leave work owed, or nothing ever removes the
/// points it just excluded.
#[test]
fn narrowing_the_filter_owes_a_pass_that_removes_the_excluded_points() {
    let (root, config, index, lesson, note) = two_types("embed-types-narrow");

    set_embed_types(&root, &["agent.lesson.v1"]);

    assert!(
        outstanding_for_tests(&root),
        "the gate saw nothing to do, so the excluded vectors would have stayed forever"
    );
    execute(&root, &config, &index, || Ok(embedder(&config, None))).expect("sync after narrowing");
    let state = index.inner.lock().unwrap();
    assert!(
        state.points.contains_key(&lesson),
        "the embedded type lost its point"
    );
    assert!(
        !state.points.contains_key(&note),
        "a type the store stopped embedding kept answering searches"
    );
    drop(state);

    // And the pass settles: nothing is owed, and a second one would not load a
    // model even if asked to.
    assert!(!outstanding_for_tests(&root), "the pass did not settle");
    execute(&root, &config, &index, || {
        panic!("a settled store loaded the embedding model");
        #[allow(unreachable_code)]
        Ok(embedder(&config, None))
    })
    .expect("no-op sync");
    fs::remove_dir_all(root).expect("remove store");
}

/// And widening it back, which is the same defect in the other direction: the
/// corpus grew and the markers alone cannot tell.
#[test]
fn widening_the_filter_owes_a_pass_that_indexes_what_it_readmits() {
    let (root, config, index, _, note) = two_types("embed-types-widen");
    set_embed_types(&root, &["agent.lesson.v1"]);
    execute(&root, &config, &index, || Ok(embedder(&config, None))).expect("sync after narrowing");
    assert!(!index.inner.lock().unwrap().points.contains_key(&note));

    set_embed_types(&root, &["agent.lesson.v1", NOTE]);

    assert!(
        outstanding_for_tests(&root),
        "widening the filter left the index looking current with a record missing"
    );
    execute(&root, &config, &index, || Ok(embedder(&config, None))).expect("sync after widening");
    assert!(
        index.inner.lock().unwrap().points.contains_key(&note),
        "the readmitted type was never indexed"
    );
    fs::remove_dir_all(root).expect("remove store");
}

/// Rewriting the same list is not a change, and must not manufacture work: a
/// configure that always published would put every store into a pass it does
/// not need, which is the cost this ticket exists to remove.
#[test]
fn rewriting_the_same_filter_owes_nothing() {
    let (root, config, index, _, _) = two_types("embed-types-idempotent");
    set_embed_types(&root, &["agent.lesson.v1"]);
    execute(&root, &config, &index, || Ok(embedder(&config, None))).expect("sync");
    assert!(!outstanding_for_tests(&root));

    set_embed_types(&root, &["agent.lesson.v1"]);

    assert!(
        !outstanding_for_tests(&root),
        "rewriting an unchanged filter asked for a pass nobody needs"
    );
    fs::remove_dir_all(root).expect("remove store");
}

/// A filter naming a type nobody registered would never match, so the saving it
/// promises never happens while the cost stays. It fails where it is written.
#[test]
fn a_filter_naming_an_unregistered_type_is_refused() {
    let (root, _, _, _, _) = two_types("embed-types-unknown");
    let mut descriptor = stored(&root);
    descriptor["embed_types"] = json!(["agent.lesson.v1", "agent.lessson.v1"]);
    let file = root.join("candidate.json");
    fs::write(&file, serde_json::to_vec(&descriptor).expect("json")).expect("candidate");

    let error = crate::vector::configure(&root, &file, "owner")
        .expect_err("a misspelled type name was accepted");

    assert!(
        error.to_string().contains("agent.lessson.v1"),
        "the error does not name the type that is wrong: {error}"
    );
    fs::remove_dir_all(root).expect("remove store");
}

fn stored(root: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(root.join("registry/vector/qdrant.json")).expect("descriptor"))
        .expect("descriptor json")
}

fn register_note(root: &Path) {
    schema::register(
        root,
        TypeDefinition {
            type_name: NOTE.into(),
            uri: "equill://agent.note/v1".into(),
            owner: "owner".into(),
            payload_schema: json!({
                "type": "object",
                "properties": { "text": { "type": "string" } },
                "required": ["text"],
                "additionalProperties": false
            }),
            lifecycle: Default::default(),
        },
        "owner",
    )
    .expect("register note type");
}

fn add_note(root: &Path, text: &str) -> Uuid {
    append(
        root,
        RecordDraft {
            namespace: "agent.memory".into(),
            type_name: NOTE.into(),
            observed_at: "2026-01-02T00:00:00Z".into(),
            valid_at: None,
            payload: json!({ "text": text }),
            evidence: Vec::new(),
            tags: Vec::new(),
            supersedes: None,
        },
        "owner",
    )
    .expect("append note")
    .id
}
