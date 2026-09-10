//! What a change to the document cap does to an index that already exists.
//!
//! The corpus digest cannot see a cap: it hashes ledger lines, and those do not
//! change when a limit does. So without the cap in the marker's identity a
//! store whose cap moved would read as current over vectors computed from other
//! text — the mixing this was asked to prevent.
//!
//! The other half of the promise is that nothing is re-embedded for nothing: a
//! record whose canonical text was never near the cap keeps the digest it had,
//! and the delta pass reuses its vector.
use super::super::{embedder, fixture};
use super::types::stored;
use crate::vector::catchup::drain::outstanding_for_tests;
use crate::vector::operator::execute;
use serde_json::json;
use std::fs;
use std::path::Path;

fn with_document_cap(root: &Path, chars: usize) {
    let mut descriptor = stored(root);
    descriptor["max_document_chars"] = json!(chars);
    let file = root.join("candidate.json");
    fs::write(&file, serde_json::to_vec(&descriptor).expect("json")).expect("candidate");
    crate::vector::configure(root, &file, "owner").expect("configure");
}

fn reloaded(root: &Path) -> crate::vector::VectorConfig {
    crate::vector::config::load(root)
        .expect("config")
        .expect("configured")
}

/// A narrowed cap leaves work owed, and the marker is what says so.
#[test]
fn narrowing_the_document_cap_owes_a_pass() {
    let (root, config, index) = fixture("caps-narrow");
    execute(&root, &config, &index, || Ok(embedder(&config, None))).expect("settle");
    assert!(
        !outstanding_for_tests(&root),
        "the premise did not hold: work was owed before the cap changed"
    );

    with_document_cap(&root, 64);

    assert!(
        outstanding_for_tests(&root),
        "a cap change left the index reading as current over other text"
    );
    let narrowed = reloaded(&root);
    execute(&root, &narrowed, &index, || Ok(embedder(&narrowed, None)))
        .expect("sync under the new cap");
    assert!(!outstanding_for_tests(&root), "the pass did not settle");
    fs::remove_dir_all(root).expect("remove store");
}

/// And a record that was never near the cap is not re-embedded for it: the
/// fixture's record is short, so the pass that follows a cap change has nothing
/// to send. Measured by the embedder, not assumed — the factory panics if it is
/// asked to load.
#[test]
fn a_record_under_both_caps_is_not_re_embedded() {
    let (root, config, index) = fixture("caps-reuse");
    execute(&root, &config, &index, || Ok(embedder(&config, None))).expect("settle");

    // Widened rather than narrowed: the record is far under either number, so
    // its canonical text — and therefore its input digest — is identical.
    with_document_cap(&root, 8_000);
    assert!(
        outstanding_for_tests(&root),
        "the premise did not hold: the marker did not notice the cap change"
    );

    let widened = reloaded(&root);
    execute(&root, &widened, &index, || {
        panic!("a record under both caps was re-embedded");
        #[allow(unreachable_code)]
        Ok(embedder(&widened, None))
    })
    .expect("pass with nothing to embed");
    assert!(!outstanding_for_tests(&root), "the pass did not settle");
    fs::remove_dir_all(root).expect("remove store");
}

/// A marker written before the cap existed is not readable as any cap, so such
/// a store is asked for one pass. Cheap where nothing was truncated, and the
/// only honest answer where something was.
#[test]
fn a_marker_without_a_cap_is_asked_for_one_pass() {
    let (root, config, index) = fixture("caps-legacy");
    execute(&root, &config, &index, || Ok(embedder(&config, None))).expect("settle");
    let path = root.join("projections/qdrant/state.json");
    let mut marker: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).expect("marker")).expect("json");
    assert!(
        marker
            .as_object_mut()
            .expect("marker object")
            .remove("max_document_chars")
            .is_some(),
        "a real pass did not record the cap, so this measures nothing"
    );
    fs::write(&path, serde_json::to_vec(&marker).expect("bytes")).expect("write marker");

    // Asked of the marker directly. `outstanding` would also need a published
    // target, and this case is about identity, not about the watermark.
    assert_eq!(
        crate::vector::freshness_of(&root)
            .expect("freshness")
            .freshness,
        crate::vector::VectorFreshness::Unknown,
        "a marker from before the cap existed was read as describing this preprocessing"
    );
    fs::remove_dir_all(root).expect("remove store");
}

/// The query cap is not part of the index's identity. Shortening a question
/// must not re-embed a corpus.
#[test]
fn changing_only_the_query_cap_owes_nothing() {
    let (root, config, index) = fixture("caps-query-only");
    execute(&root, &config, &index, || Ok(embedder(&config, None))).expect("settle");

    let mut descriptor = stored(&root);
    descriptor["max_query_chars"] = json!(128);
    let file = root.join("candidate.json");
    fs::write(&file, serde_json::to_vec(&descriptor).expect("json")).expect("candidate");
    crate::vector::configure(&root, &file, "owner").expect("configure");

    assert!(
        !outstanding_for_tests(&root),
        "a query cap change asked for a pass over the documents"
    );
    fs::remove_dir_all(root).expect("remove store");
}
