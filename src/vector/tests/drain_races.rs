//! The interleavings a catch-up has to survive.
use super::drain::{add, configure_unreachable, store};
use crate::vector::after_commit;
use std::fs;

/// The regression this exists to prevent: two catch-ups interleaving so that
/// one publishes a target the other has already passed. A watermark that walks
/// backwards leaves the drain comparing an index at N+1 against a target of N,
/// which it can never satisfy — a loop that never agrees with itself.
#[test]
fn a_published_target_never_walks_backwards() {
    let root = store("monotonic");
    configure_unreachable(&root);
    add(&root, "first");
    add(&root, "second");
    after_commit(&root);
    let reached = super::super::desired::read(&root)
        .expect("desired")
        .expect("published")
        .records;

    // A stale publisher, holding a snapshot from before those writes, tries to
    // record what it saw. It must not undo what has already been reached.
    super::super::desired::publish(&root, reached - 1, &"a".repeat(64)).expect("stale publish");
    let after_stale = super::super::desired::read(&root)
        .expect("desired")
        .expect("published");

    assert_eq!(after_stale.records, reached, "a stale target is ignored");
    // Moving forward still works, which is the point of the guard.
    super::super::desired::publish(&root, reached + 1, &"b".repeat(64)).expect("forward publish");
    assert_eq!(
        super::super::desired::read(&root)
            .expect("desired")
            .expect("published")
            .records,
        reached + 1
    );
    fs::remove_dir_all(root).expect("cleanup");
}

/// Concurrent writers must leave the watermark describing the ledger as it
/// actually is, not as whichever of them read it last happened to see.
#[test]
fn concurrent_catch_ups_agree_on_the_ledger_they_left_behind() {
    let root = store("concurrent");
    configure_unreachable(&root);
    let writers = (0..4)
        .map(|index| {
            let root = root.clone();
            std::thread::spawn(move || {
                add(&root, &format!("rule {index}"));
                after_commit(&root);
            })
        })
        .collect::<Vec<_>>();
    for writer in writers {
        writer.join().expect("writer thread");
    }

    let published = super::super::desired::read(&root)
        .expect("desired")
        .expect("published");
    let ledger = crate::record::read_all(&root).expect("records").len();

    assert_eq!(ledger, 4, "every write landed");
    assert_eq!(
        published.records, ledger,
        "the target describes the ledger, whoever published last"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

/// What makes the holder loop instead of leaving: a write that lands during an
/// active drain moves the target past what is indexed, and the holder's exit
/// condition must then be false. The previous busy test was sequential — every
/// `add` already drains — so it never exercised this at all.
#[test]
fn a_write_during_a_drain_denies_the_exit_condition() {
    let root = store("second-pass");
    configure_unreachable(&root);
    add(&root, "already indexed");
    // Pretend the drain got this far: the checkpoint matches the ledger.
    let (records, digest) = super::super::operator::corpus(&root).expect("corpus");
    let config = super::super::config::load(&root)
        .expect("config")
        .expect("configured");
    super::super::state::stage_ready(
        &root,
        &config,
        "equill_drain_test_physical",
        Some((records.len(), &digest)),
    )
    .expect("stage")
    .commit()
    .expect("commit");
    super::super::desired::publish(&root, records.len(), &digest).expect("publish");
    let settled = super::super::drain::caught_up(&root).expect("caught up");

    // Now a write lands while the holder still owns the drain lock.
    add(&root, "arrived mid-drain");
    let (after, after_digest) = super::super::operator::corpus(&root).expect("corpus");
    super::super::desired::publish(&root, after.len(), &after_digest).expect("publish");
    let unsettled = super::super::drain::caught_up(&root).expect("caught up");

    assert!(settled, "with nothing outstanding the holder may leave");
    assert!(
        !unsettled,
        "with a tail outstanding the holder must take another pass"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

/// Running a sync on demand is governance and stays with the owner. Reaching
/// the same work as a consequence of a write one was already allowed to make is
/// not — and requiring root there denied every scoped writer the index their
/// own writes had just changed, with no way for them to fix it.
#[test]
fn a_scoped_writer_reaches_the_catch_up_that_its_own_write_needs() {
    let root = store("scoped");
    configure_unreachable(&root);
    grant_scoped_writer(&root, "finding-agent");

    let scoped = crate::record::append(&root, draft("written by a scoped writer"), "finding-agent")
        .expect("a scoped writer may append");
    let manual = crate::vector::sync(&root, "finding-agent");
    let owner_manual = crate::vector::sync(&root, "owner");

    // The write succeeded, and its catch-up was attempted: what stopped it is
    // the unreachable provider, not an authorization refusal.
    let attempt = scoped.vector.attempt_error.expect("an attempt was made");
    assert!(
        !attempt.contains("denied") && !attempt.contains("not allowed"),
        "the internal catch-up must not re-authorize the writer: {attempt}"
    );
    // The public command is unchanged: still owner-only.
    assert!(
        matches!(manual, Err(crate::kernel::error::Error::PermissionDenied)),
        "manual sync stays governance"
    );
    // And for the owner it fails on the provider, not on permission.
    assert!(!matches!(
        owner_manual,
        Err(crate::kernel::error::Error::PermissionDenied)
    ));
    fs::remove_dir_all(root).expect("cleanup");
}

/// The store-wide legacy writer is the same case and costs one line to cover.
#[test]
fn a_legacy_store_wide_writer_reaches_it_too() {
    let root = store("legacy");
    configure_unreachable(&root);
    grant_legacy_writer(&root, "legacy-agent");

    let written = crate::record::append(&root, draft("written by a legacy writer"), "legacy-agent")
        .expect("a legacy writer may append");

    let attempt = written.vector.attempt_error.expect("an attempt was made");
    assert!(
        !attempt.contains("denied") && !attempt.contains("not allowed"),
        "{attempt}"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

fn draft(rule: &str) -> crate::record::RecordDraft {
    crate::record::RecordDraft {
        namespace: "agent.memory".into(),
        type_name: "agent.lesson.v1".into(),
        observed_at: "2026-01-01T00:00:00Z".into(),
        valid_at: None,
        payload: serde_json::json!({ "rule": rule }),
        evidence: Vec::new(),
        tags: Vec::new(),
        supersedes: None,
    }
}

fn grant_scoped_writer(root: &std::path::Path, actor: &str) {
    edit_store(root, |config| {
        config.insert(
            "write_grants".into(),
            serde_json::json!([{
                "actors": [actor],
                "namespace": "agent.memory",
                "types": ["agent.lesson.v1"]
            }]),
        );
    });
}

fn grant_legacy_writer(root: &std::path::Path, actor: &str) {
    edit_store(root, |config| {
        config.insert("writers".into(), serde_json::json!([actor]));
    });
}

fn edit_store(
    root: &std::path::Path,
    mutate: impl Fn(&mut serde_json::Map<String, serde_json::Value>),
) {
    let path = root.join("store.json");
    let mut config: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).expect("store metadata")).expect("json");
    mutate(config.as_object_mut().expect("object"));
    fs::write(&path, serde_json::to_vec(&config).expect("json")).expect("store metadata");
}
