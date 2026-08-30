use crate::governance::tests::transfer::audit_records;
use crate::governance::tests::{interrupt, journal_exists, store};
use crate::governance::{grant, journal, metadata, recover, transfer};
use crate::kernel::error::Error;
use crate::kernel::store as config;

/// A transaction that died before it announced anything has nothing to finish.
/// Nobody was told, nothing moved: drop it.
#[test]
fn a_transaction_that_was_never_announced_is_abandoned() {
    let root = store();
    let plan = metadata::plan(&root, "founder", |config| {
        config.root_owner = "successor".into();
        Ok(())
    })
    .expect("plan");
    let tx_id = uuid::Uuid::now_v7();
    journal::write(
        &root,
        &journal::Pending {
            tx_id,
            action: "owner-transfer".into(),
            subject: "successor".into(),
            before_sha256: plan.before.clone(),
            after_sha256: plan.after.clone(),
            after_bytes: String::from_utf8(plan.bytes.clone()).expect("utf-8"),
        },
    )
    .expect("journal");

    let outcome = recover(&root).expect("recovery");

    assert_eq!(outcome, Some(format!("abandoned {tx_id}")));
    assert_eq!(config::load(&root).expect("load").root_owner, "founder");
    assert!(!journal_exists(&root));
    assert!(audit_records(&root).is_empty());
}

/// The swap landed and only the journal outlived it. Nothing to redo.
#[test]
fn a_transaction_that_already_applied_leaves_only_its_journal_to_clear() {
    let root = store();
    let tx_id = interrupt(&root, "successor");
    let pending = journal::read(&root).expect("read").expect("pending");
    metadata::write_bytes(&root, pending.after_bytes.as_bytes()).expect("apply by hand");

    let outcome = recover(&root).expect("recovery");

    assert_eq!(outcome, Some(format!("already applied {tx_id}")));
    assert_eq!(config::load(&root).expect("load").root_owner, "successor");
    assert!(!journal_exists(&root));
}

/// Metadata at a digest the transaction never described means something else
/// wrote the store. Recovery refuses, and keeps the journal: it is the only
/// evidence of what was in flight, and discarding it to make the error go away
/// would destroy the record of the damage.
#[test]
fn an_unexplainable_state_fails_integrity_and_keeps_the_evidence() {
    let root = store();
    // Move the metadata somewhere neither digest describes, while a
    // transaction that describes two other digests is pending.
    let sideways = metadata::plan(&root, "founder", |config| {
        config.namespaces.push("other.space".into());
        Ok(())
    })
    .expect("plan");
    interrupt(&root, "successor");
    metadata::write_bytes(&root, &sideways.bytes).expect("write");

    let outcome = recover(&root);

    assert!(matches!(outcome, Err(Error::Integrity(_))), "{outcome:?}");
    assert!(journal_exists(&root), "the journal is evidence, not litter");
}

/// A pending transaction is settled before any new one starts, so a store never
/// carries two.
#[test]
fn a_new_mutation_finishes_the_previous_transaction_first() {
    let root = store();
    let interrupted = interrupt(&root, "successor");

    grant(
        &root,
        "worker",
        "agent.memory",
        &["agent.lesson.v1".into()],
        None,
        "successor",
    )
    .expect("the recovered owner governs");

    assert_eq!(config::load(&root).expect("load").root_owner, "successor");
    assert!(!journal_exists(&root));
    let audit = audit_records(&root);
    assert_eq!(audit.len(), 2);
    assert_eq!(audit[0]["payload"]["tx_id"], interrupted.to_string());
    assert_ne!(audit[1]["payload"]["tx_id"], interrupted.to_string());
}

/// The reason a change was made is attested, never quoted.
#[test]
fn the_audit_carries_a_digest_of_the_comment_and_not_its_text() {
    let root = store();

    transfer(&root, "successor", Some("a private reason"), "founder").expect("transfer");

    let audit = audit_records(&root);
    let payload = &audit[0]["payload"];
    assert_eq!(
        payload["comment_sha256"],
        crate::kernel::digest::sha256_hex(b"a private reason")
    );
    assert!(payload.get("comment").is_none());
    assert!(!serde_json::to_string(payload).unwrap().contains("private"));
}

/// Governance writes its audit into a namespace the store already has, and
/// leaves the namespace list exactly as it found it.
#[test]
fn governance_invents_no_namespace_of_its_own() {
    let root = store();
    let before = config::load(&root).expect("load").namespaces;

    transfer(&root, "successor", None, "founder").expect("transfer");

    let after = config::load(&root).expect("load").namespaces;
    assert_eq!(before, after, "the namespace list is the user's, not ours");
    assert_eq!(audit_records(&root)[0]["namespace"], before[0].as_str());
}
