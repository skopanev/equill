//! What compaction leaves settled, and what it refuses to call settled.
use super::journal::{Journal, with_interrupt};
use super::plan_tests::{add, store};
use super::run::run;
use crate::record::read_all;

/// A store with no vector configured compacts and keeps working.
///
/// Named for what it measures. It does not prove the relabelling: with no
/// provider configured the catch-up has nothing to do, so removing the call
/// leaves this green. What it does prove is that the catch-up cannot fail a
/// compaction on a store that never asked for a vector.
#[test]
fn a_store_without_a_vector_compacts_and_keeps_working() {
    let root = store("vector-settled");
    let first = add(&root, "older", None);
    add(&root, "newer", Some(first));

    let report = run(&root, true, "owner").expect("compaction");
    assert_eq!(report.removed, 1);

    // And the store is caught up enough that an ordinary write lands and is
    // findable without any repair step in between.
    let written = add(&root, "written after compaction", None);
    let after = read_all(&root).expect("ledger");
    assert!(
        after.iter().any(|record| record.id == written),
        "the store did not accept a write after compaction"
    );
    assert!(
        after.iter().all(|record| record.supersedes.is_none()),
        "a dangling link survived"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// A survivor's receipt describes the bytes that are now in the ledger.
///
/// Cutting a link rewrites the record, so its stored hash moves. A receipt
/// still carrying the old one would make a verification report corruption for
/// a record nobody touched — the store accusing itself of damage it did
/// deliberately.
#[test]
fn a_rewritten_survivor_and_its_receipt_agree() {
    let root = store("receipts");
    let first = add(&root, "older", None);
    let survivor = add(&root, "newer", Some(first));

    run(&root, true, "owner").expect("compaction");

    let record = read_all(&root)
        .expect("ledger")
        .into_iter()
        .find(|record| record.id == survivor)
        .expect("survivor");
    let digest = crate::kernel::digest::sha256_hex(&serde_json::to_vec(&record).expect("bytes"));
    let month = &record.recorded_at[..7];
    let path = root
        .join("receipts/writes")
        .join(month)
        .join(format!("{survivor}.json"));
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("receipt")).expect("json");

    assert_eq!(
        receipt["record_sha256"].as_str(),
        Some(digest.as_str()),
        "the receipt still attests to bytes the ledger no longer holds"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Only the backup left, and nothing to finish with. Putting it back would
/// make the store readable and then look finished — journal cleared, a later
/// compaction running against receipts from a half-published transaction. It
/// refuses, leaving the backup and the journal where they are.
#[test]
fn a_backup_with_no_prepared_copy_is_refused_rather_than_guessed_at() {
    let root = store("backup-only");
    let first = add(&root, "older", None);
    add(&root, "newer", Some(first));

    let interrupted = with_interrupt("inside-records", || run(&root, true, "owner"));
    assert!(interrupted.is_err(), "the fixture did not interrupt");

    // Remove the prepared copy, leaving only what was set aside.
    let journal = Journal::read(&root).expect("journal").expect("a journal");
    std::fs::remove_dir_all(&journal.shadow).expect("drop the staged copy");
    let backup =
        super::super::transaction::sibling(&root.join("records"), "backup", &journal.transaction)
            .expect("backup path");
    assert!(backup.is_dir(), "the fixture has no backup to protect");

    let outcome = run(&root, true, "owner");

    assert!(
        outcome.is_err(),
        "recovery guessed at a state it could not resolve"
    );
    assert!(
        backup.is_dir(),
        "the backup was consumed by a failed recovery"
    );
    assert!(
        Journal::read(&root).expect("journal").is_some(),
        "the journal was cleared while the transaction was unresolved"
    );
    let _ = std::fs::remove_dir_all(&root);
}
