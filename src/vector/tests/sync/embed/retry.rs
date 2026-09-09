//! What a configure that failed halfway leaves for the next attempt.
use super::super::embedder;
use super::types::{stored, two_types};
use crate::vector::catchup::drain::outstanding_for_tests;
use crate::vector::operator::execute;
use serde_json::json;
use std::fs;
use std::path::Path;

/// The retry a failed publication leaves behind.
///
/// Storing the descriptor first opened a window with no exit: the filter was
/// saved, the target failed, and a retry of the same file compared the new
/// filter against itself, found no change and returned success. The store then
/// read as configured and settled while the excluded vectors kept answering,
/// and no further attempt could correct it — the operator had no failure left
/// to react to.
#[test]
fn a_failed_target_publication_leaves_a_retry_that_still_owes_the_pass() {
    let (root, config, index, _, note) = two_types("embed-types-retry");
    let markers = root.join("projections/qdrant");
    let descriptor_before = stored(&root);

    // The registry stays writable; only the marker directory refuses a new file,
    // which is exactly the half that fails when the target cannot be published.
    seal(&markers);
    let failure = try_set_embed_types(&root, &["agent.lesson.v1"]);
    unseal(&markers);

    assert!(
        failure.is_err(),
        "the premise did not hold: publication succeeded, so this measures nothing"
    );
    // Compared as a value rather than as bytes: the undo rewrites the file
    // through the ordinary serializer, so the formatting can differ while the
    // descriptor a loader sees is the one that was there before.
    assert_eq!(
        stored(&root),
        descriptor_before,
        "the descriptor was stored while the target was not, which is the state that cannot be retried out of"
    );

    // The same candidate again, now that the store can write markers.
    try_set_embed_types(&root, &["agent.lesson.v1"]).expect("retry");

    assert!(
        outstanding_for_tests(&root),
        "the retry reported success while leaving the index reading as current"
    );
    execute(&root, &config, &index, || Ok(embedder(&config, None))).expect("sync after retry");
    assert!(
        !index.inner.lock().unwrap().points.contains_key(&note),
        "the excluded point survived a configure that reported success"
    );
    assert!(!outstanding_for_tests(&root), "the pass did not settle");
    execute(&root, &config, &index, || {
        panic!("a settled store loaded the embedding model");
        #[allow(unreachable_code)]
        Ok(embedder(&config, None))
    })
    .expect("no-op sync");
    fs::remove_dir_all(root).expect("remove store");
}

fn try_set_embed_types(root: &Path, types: &[&str]) -> Result<(), crate::kernel::error::Error> {
    let mut descriptor = stored(root);
    descriptor["embed_types"] = json!(types);
    let file = root.join("candidate.json");
    fs::write(&file, serde_json::to_vec(&descriptor).expect("json")).expect("candidate");
    crate::vector::configure(root, &file, "owner").map(|_| ())
}

/// Read-only, so the existing markers stay readable and only a new file fails.
fn seal(directory: &Path) {
    permissions(directory, 0o500);
}

fn unseal(directory: &Path) {
    permissions(directory, 0o700);
}

fn permissions(directory: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(directory, fs::Permissions::from_mode(mode)).expect("permissions");
}

/// Why the descriptor is stored before its target, and not the other way round.
///
/// An automatic sync reads the target before the corpus and holds no lock while
/// it does — deliberately, because hashing the ledger under the writer lock made
/// every concurrent write wait. So a pass can slip between the two halves of a
/// configure.
///
/// Since the checkpoint carries the filter its corpus was taken under, neither
/// order can leave a false current any more: a pass that lands between the
/// halves stamps the filter it actually applied, and that stamp stops matching
/// the configuration as soon as the other half lands. What the order still buys
/// is one avoided pass, not a correct answer. Both are run here against the
/// same fixture so that difference is visible rather than asserted.
#[test]
fn a_sync_between_the_two_halves_of_a_configure_leaves_the_pass_owed() {
    let (root, config, index, _, note) = two_types("embed-types-interleave");
    let narrowed = narrowed(&root);

    // Descriptor first, target second — the order `configure` uses. The pass in
    // between covers the narrowed corpus against the old target, so it stays
    // owed and runs again.
    let before = Some(stored(&root));
    crate::vector::operator::store_descriptor(&root, &narrowed, before).expect("descriptor");
    execute(&root, &config, &index, || Ok(embedder(&config, None))).expect("interleaved sync");
    crate::vector::operator::announce_outstanding_work(&root).expect("target");

    assert!(
        outstanding_for_tests(&root),
        "a sync landing between the two halves swallowed the target, and no further pass would run"
    );
    execute(&root, &config, &index, || Ok(embedder(&config, None))).expect("sync after configure");
    assert!(!index.inner.lock().unwrap().points.contains_key(&note));
    fs::remove_dir_all(root).expect("remove store");
}

/// The same interleaving with the halves swapped — the order this code does not
/// use, and which used to lose the tail.
///
/// It no longer can, and the reason is worth stating: the pass that slips
/// between the halves takes its corpus under the OLD descriptor, so it stamps
/// its checkpoint with the old filter. When the new descriptor lands, that
/// stamp no longer matches the configuration, the checkpoint stops describing
/// this store, and the work is owed however the target happens to compare. The
/// excluded point is still in the collection at that moment — the pass that ran
/// had no filter to apply — and the next pass under the current configuration
/// removes it.
///
/// So the protection sits in the marker's identity, not in the order of the two
/// writes. The order is still the one worth keeping, because it avoids a wasted
/// pass rather than a wrong answer; this test no longer carries the argument
/// for it.
#[test]
fn the_reversed_order_still_owes_the_pass_that_removes_the_excluded_point() {
    let (root, config, index, _, note) = two_types("embed-types-interleave-reversed");
    let narrowed = narrowed(&root);
    let before = Some(stored(&root));

    crate::vector::operator::announce_outstanding_work(&root).expect("target");
    execute(&root, &config, &index, || Ok(embedder(&config, None))).expect("interleaved sync");
    crate::vector::operator::store_descriptor(&root, &narrowed, before).expect("descriptor");

    assert!(
        outstanding_for_tests(&root),
        "the reversed order left the index reading as current over a corpus that had changed"
    );
    assert!(
        index.inner.lock().unwrap().points.contains_key(&note),
        "the premise did not hold: the interleaved pass already applied a filter it could not see"
    );

    // The pass that follows reads the descriptor that is there now.
    let current = crate::vector::config::load(&root)
        .expect("config")
        .expect("configured");
    execute(&root, &current, &index, || Ok(embedder(&current, None)))
        .expect("sync under the current configuration");

    assert!(
        !index.inner.lock().unwrap().points.contains_key(&note),
        "the excluded point survived the pass that was owed"
    );
    assert!(!outstanding_for_tests(&root), "the pass did not settle");
    fs::remove_dir_all(root).expect("remove store");
}

fn narrowed(root: &Path) -> serde_json::Value {
    let mut descriptor = stored(root);
    descriptor["embed_types"] = json!(["agent.lesson.v1"]);
    descriptor
}

/// The undo can fail too, and the swallowed version of it is worse than the
/// failure it hides: the operator reads an error about the target, fixes the
/// disk, retries the same file, and gets a success over a store whose filter is
/// new and whose target is old. What comes back has to say what the store is.
#[test]
fn an_undo_that_cannot_write_says_so_instead_of_reporting_the_target_error() {
    let (root, _, _, _, _) = two_types("embed-types-undo-fails");
    let previous = stored(&root);
    // The descriptor file itself, not its directory: overwriting an existing
    // file asks permission of the file, and a read-only directory would leave
    // the undo succeeding while the test believed it had blocked it.
    let descriptor = root.join("registry/vector/qdrant.json");
    permissions(&descriptor, 0o400);

    let error = crate::vector::operator::undo(&root, Some(previous))
        .expect_err("an undo that could not write reported success");

    permissions(&descriptor, 0o600);
    let message = error.to_string();
    assert!(
        message.contains("could not be restored") && message.contains("read as current"),
        "the error does not say what the store is now: {message}"
    );
    fs::remove_dir_all(root).expect("remove store");
}
