use super::super::{metadata, transfer};
use super::store;
use crate::kernel::error::Error;
use crate::kernel::identity;
use crate::kernel::store as config;

/// The point of a handover: the previous owner stops governing, and the new one
/// starts. Anything less is a label change.
#[test]
fn a_handover_moves_root_and_takes_the_old_store_wide_append_with_it() {
    let root = store();

    let report = transfer(&root, "successor", Some("planned"), "founder")
        .expect("the owner may hand the store over");
    let after = config::load(&root).expect("load");

    assert_eq!(report.previous_owner, "founder");
    assert_eq!(report.owner, "successor");
    assert_eq!(
        report.revoked_writers,
        vec!["store-wide append".to_string()]
    );
    assert_eq!(after.root_owner, "successor");
    assert!(!after.writers.iter().any(|writer| writer == "founder"));
    // The unrelated writer is left exactly as it was: a handover is not a purge.
    assert!(after.writers.iter().any(|writer| writer == "legacy"));
    assert!(identity::require_root(&after, "founder").is_err());
    identity::require_root(&after, "successor").expect("the new owner governs");
}

/// A handover takes every form of append the old owner had, scoped grants
/// included. Leaving one behind would let them keep writing what they were
/// granted, which is the same hole in a smaller shape.
#[test]
fn a_handover_takes_the_old_owners_scoped_grants_too() {
    let root = store();
    super::super::grant(
        &root,
        "founder",
        "agent.memory",
        &["agent.lesson.v1".into()],
        None,
        "founder",
    )
    .expect("the owner may hold a scoped grant like anyone else");

    let report = transfer(&root, "successor", None, "founder").expect("transfer");
    let after = config::load(&root).expect("load");

    assert!(
        report
            .revoked_writers
            .contains(&"store-wide append".to_string())
    );
    assert!(
        report
            .revoked_writers
            .contains(&"1 scoped grant".to_string())
    );
    assert!(
        after.write_grants.is_empty(),
        "the grant went with the role"
    );
    assert!(
        identity::require_type_writer(&after, "founder", "agent.memory", "agent.lesson.v1")
            .is_err()
    );
}

/// The ledger states which metadata digest the store moved to, so the two can
/// be checked against each other rather than trusted separately.
#[test]
fn the_ledger_states_the_metadata_digest_the_store_actually_reached() {
    let root = store();

    let report = transfer(&root, "successor", None, "founder").expect("transfer");
    let live = metadata::digest(&root).expect("digest");

    assert_eq!(report.store_sha256, live);
    let audit = audit_records(&root);
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0]["payload"]["action"], "owner-transfer");
    assert_eq!(audit[0]["payload"]["subject"], "successor");
    assert_eq!(audit[0]["payload"]["store_sha256_after"], live);
    assert_ne!(
        audit[0]["payload"]["store_sha256_before"],
        audit[0]["payload"]["store_sha256_after"]
    );
    // Hash-only: the audit never carries the authority itself.
    assert!(audit[0]["payload"].get("writers").is_none());
}

/// A crash between the audit and the metadata commit must leave a store that
/// still loads and still obeys its old owner, and must be finishable rather
/// than merely repeatable: the journal names the transaction, so recovery
/// completes the move the audit already announced instead of starting a second
/// one.
#[test]
fn an_interrupted_transaction_is_completed_by_recovery_without_a_second_audit() {
    let root = store();
    let interrupted = super::interrupt(&root, "successor");

    let before_recovery = config::load(&root).expect("the store still loads");
    assert_eq!(before_recovery.root_owner, "founder");
    assert!(no_temporary_metadata(&root), "staging left a file behind");
    assert_eq!(audit_records(&root).len(), 1, "the intent was announced");

    let outcome = super::super::recover(&root).expect("recovery");

    assert_eq!(outcome, Some(format!("completed {interrupted}")));
    assert_eq!(config::load(&root).expect("load").root_owner, "successor");
    assert_eq!(
        audit_records(&root).len(),
        1,
        "finishing a transaction writes no second audit record"
    );
    let live = metadata::digest(&root).expect("digest");
    assert_eq!(
        audit_records(&root)[0]["payload"]["store_sha256_after"],
        live
    );
    assert!(!super::journal_exists(&root));
}

/// Two handovers planned against the same state: the second cannot land on
/// metadata the first already moved, or it would silently undo it.
#[test]
fn a_plan_refuses_to_land_on_metadata_that_moved_underneath_it() {
    let root = store();

    let stale = metadata::plan(&root, "founder", |config| {
        config.root_owner = "first".into();
        Ok(())
    })
    .expect("plan");
    transfer(&root, "second", None, "founder").expect("another handover lands first");

    assert!(matches!(
        metadata::commit(&root, &stale),
        Err(Error::StoreMismatch)
    ));
    assert_eq!(config::load(&root).expect("load").root_owner, "second");
}

#[test]
fn a_transfer_to_the_current_owner_or_an_invalid_identity_is_refused() {
    let root = store();

    assert!(matches!(
        transfer(&root, "founder", None, "founder"),
        Err(Error::InvalidOwner)
    ));
    assert!(matches!(
        transfer(&root, "  ", None, "founder"),
        Err(Error::InvalidOwner)
    ));
    assert!(matches!(
        transfer(&root, "successor", None, "legacy"),
        Err(Error::PermissionDenied)
    ));
    assert_eq!(config::load(&root).expect("load").root_owner, "founder");
    assert!(
        audit_records(&root).is_empty(),
        "a refused call writes no history"
    );
}

pub(super) fn audit_records(root: &std::path::Path) -> Vec<serde_json::Value> {
    let mut records = Vec::new();
    let Ok(entries) = std::fs::read_dir(root.join("records")) else {
        return records;
    };
    for entry in entries.flatten() {
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let value: serde_json::Value = serde_json::from_str(line).expect("record");
            if value["type"] == "equill.governance.v1" {
                records.push(value);
            }
        }
    }
    records.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
    records
}

fn no_temporary_metadata(root: &std::path::Path) -> bool {
    std::fs::read_dir(root)
        .expect("store directory")
        .flatten()
        .all(|entry| !entry.file_name().to_string_lossy().starts_with(".store-"))
}
