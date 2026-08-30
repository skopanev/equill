use crate::governance::tests::transfer::audit_records;
use crate::governance::tests::{interrupt, journal_exists, store};
use crate::governance::{grant, journal, metadata, recover, transfer};
use crate::kernel::error::Error;
use crate::kernel::store as config;

/// A change that would produce metadata the store cannot load must fail before
/// anything is written. The old failure was worse than a rejected call: the
/// write succeeded and every later open of the store failed.
#[test]
fn a_grant_naming_an_unknown_namespace_leaves_the_store_untouched() {
    let root = store();
    let before = std::fs::read(root.join("store.json")).expect("metadata");

    let refused = grant(
        &root,
        "worker",
        "missing.space",
        &["agent.lesson.v1".into()],
        None,
        "founder",
    );

    assert!(
        matches!(refused, Err(Error::InvalidNamespace)),
        "{refused:?}"
    );
    assert_eq!(
        std::fs::read(root.join("store.json")).expect("metadata"),
        before,
        "a refused grant must not alter the metadata by a single byte"
    );
    config::load(&root).expect("the store still loads");
    assert!(audit_records(&root).is_empty(), "and announces nothing");
    assert!(!journal_exists(&root));
}

/// The journal sits in an ordinary file. Anything that can write to the store
/// can edit it, so recovery must not act on it — only on what the immutable
/// ledger attested.
#[test]
fn recovery_refuses_journal_bytes_that_the_ledger_never_attested() {
    let root = store();
    interrupt(&root, "successor");
    let pending = journal::read(&root).expect("read").expect("pending");
    // Valid metadata, valid JSON, a different owner — and the declared digest
    // left untouched, so only hashing the bytes can catch it.
    let forged = pending.after_bytes.replace("successor", "attacker");
    assert_ne!(forged, pending.after_bytes);
    let path = crate::governance::journal::path(&root);
    let tampered = pending.tampered_with(&forged);
    std::fs::write(&path, serde_json::to_vec(&tampered).expect("json")).expect("write");

    let outcome = recover(&root);

    assert!(matches!(outcome, Err(Error::Integrity(_))), "{outcome:?}");
    assert_eq!(
        config::load(&root).expect("load").root_owner,
        "founder",
        "the forged owner must never reach the store"
    );
    assert!(journal_exists(&root), "the tampered journal is evidence");
}

/// The same defence one level up: a journal whose declared intent disagrees with
/// the audit record for its transaction is not a usable instruction, whatever
/// its bytes hash to.
#[test]
fn recovery_refuses_a_journal_that_disagrees_with_the_ledger() {
    let root = store();
    interrupt(&root, "successor");
    let pending = journal::read(&root).expect("read").expect("pending");
    let mut altered = pending;
    altered.subject = "attacker".into();
    std::fs::write(
        crate::governance::journal::path(&root),
        serde_json::to_vec(&altered).expect("json"),
    )
    .expect("write");

    let outcome = recover(&root);

    assert!(matches!(outcome, Err(Error::Integrity(_))), "{outcome:?}");
    assert_eq!(config::load(&root).expect("load").root_owner, "founder");
    assert!(journal_exists(&root));
}

/// A file already sitting under the engine's audit type is not accepted just
/// because it exists: governance history written against somebody else's schema
/// could not be verified afterwards.
#[test]
fn a_foreign_schema_under_the_engine_audit_type_is_refused() {
    let root = store();
    let path = root.join("registry/types/equill.governance.v1.json");
    std::fs::create_dir_all(path.parent().expect("parent")).expect("registry");
    std::fs::write(
        &path,
        serde_json::to_vec(&serde_json::json!({
            "type": "equill.governance.v1",
            "uri": "equill://equill.governance/v1",
            "owner": "founder",
            "payload_schema": { "type": "object" }
        }))
        .expect("json"),
    )
    .expect("write");

    let refused = transfer(&root, "successor", None, "founder");

    assert!(matches!(refused, Err(Error::Integrity(_))), "{refused:?}");
    assert_eq!(config::load(&root).expect("load").root_owner, "founder");
}

/// Governance records are excluded from the vector corpus, so a handover does
/// not silently leave every store lagging until somebody runs a sync.
#[test]
fn a_governance_record_does_not_move_the_vector_corpus() {
    let root = store();
    let (before, before_digest) = crate::vector::corpus(&root).expect("corpus");

    transfer(&root, "successor", None, "founder").expect("transfer");

    let (after, after_digest) = crate::vector::corpus(&root).expect("corpus");
    assert_eq!(audit_records(&root).len(), 1, "the record was written");
    assert_eq!(before.len(), after.len(), "but it is not embeddable");
    assert_eq!(before_digest, after_digest, "so the corpus did not move");
}

/// And the metadata digest the report hands back is the one the store is
/// actually at, after every one of these defences has run.
#[test]
fn a_completed_transaction_leaves_no_journal_and_a_matching_digest() {
    let root = store();

    let report = transfer(&root, "successor", None, "founder").expect("transfer");

    assert!(!journal_exists(&root));
    assert_eq!(
        report.store_sha256,
        metadata::digest(&root).expect("digest")
    );
}

/// The regression Codex found: when the store has ALREADY reached the state the
/// transaction describes, recovery used to clear the journal without ever
/// looking at its bytes. Clearing is itself an action on a tampered journal — it
/// destroys the only evidence that a forgery was attempted — so the bytes must
/// be proved before that branch is taken.
#[test]
fn a_tampered_journal_is_kept_even_when_the_store_already_reached_that_state() {
    let root = store();
    interrupt(&root, "successor");
    let pending = journal::read(&root).expect("read").expect("pending");
    // Let the transaction land, so live == the attested after digest.
    metadata::write_bytes(&root, pending.after_bytes.as_bytes()).expect("apply");
    assert_eq!(
        metadata::digest(&root).expect("digest"),
        pending.after_sha256
    );
    // Now forge the bytes, leaving the declared digests alone.
    let forged = pending.after_bytes.replace("successor", "attacker");
    assert_ne!(forged, pending.after_bytes);
    std::fs::write(
        crate::governance::journal::path(&root),
        serde_json::to_vec(&pending.tampered_with(&forged)).expect("json"),
    )
    .expect("write");

    let outcome = recover(&root);

    assert!(matches!(outcome, Err(Error::Integrity(_))), "{outcome:?}");
    assert!(
        journal_exists(&root),
        "the tampered journal is evidence and must survive"
    );
    assert_eq!(
        config::load(&root).expect("load").root_owner,
        "successor",
        "and the applied state is left exactly as it was"
    );
}

/// A journal that was never announced is still vetted before it is dropped: a
/// forgery attempt leaves a trace even when it could never have succeeded.
#[test]
fn an_unannounced_journal_is_vetted_before_it_is_abandoned() {
    let root = store();
    let plan = metadata::plan(&root, "founder", |config| {
        config.root_owner = "successor".into();
        Ok(())
    })
    .expect("plan");
    let pending = journal::Pending {
        tx_id: uuid::Uuid::now_v7(),
        action: "owner-transfer".into(),
        subject: "successor".into(),
        before_sha256: plan.before.clone(),
        after_sha256: plan.after.clone(),
        after_bytes: String::from_utf8(plan.bytes.clone()).expect("utf-8"),
    };
    let forged = pending.after_bytes.replace("successor", "attacker");
    let path = crate::governance::journal::path(&root);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("journal directory");
    std::fs::write(
        &path,
        serde_json::to_vec(&pending.tampered_with(&forged)).expect("json"),
    )
    .expect("write");

    let outcome = recover(&root);

    assert!(matches!(outcome, Err(Error::Integrity(_))), "{outcome:?}");
    assert!(journal_exists(&root));
    assert_eq!(config::load(&root).expect("load").root_owner, "founder");
}
