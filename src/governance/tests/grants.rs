use super::super::{grant, revoke_grant, show};
use super::store;
use super::transfer::audit_records;
use crate::kernel::error::Error;
use crate::kernel::identity;
use crate::kernel::store as config;

/// A grant is least privilege: the named namespace and types, nothing beside
/// them, and no governance at all.
#[test]
fn a_grant_opens_exactly_what_it_names_and_nothing_else() {
    let root = store();

    let report = grant(
        &root,
        "worker",
        "agent.memory",
        &["agent.lesson.v1".into()],
        Some("scoped"),
        "founder",
    )
    .expect("grant");
    let after = config::load(&root).expect("load");

    assert!(report.changed);
    assert_eq!(report.grants, 1);
    identity::require_type_writer(&after, "worker", "agent.memory", "agent.lesson.v1")
        .expect("the granted type");
    assert!(
        identity::require_type_writer(&after, "worker", "agent.memory", "agent.finding.v1")
            .is_err()
    );
    assert!(identity::require_root(&after, "worker").is_err());
}

/// Writing the same grant twice is not an error and is not a second grant. The
/// caller gets told nothing changed rather than being made to check first.
#[test]
fn repeating_a_grant_changes_nothing_and_says_so() {
    let root = store();
    let types = ["agent.lesson.v1".to_string()];

    grant(&root, "worker", "agent.memory", &types, None, "founder").expect("first");
    let again = grant(&root, "worker", "agent.memory", &types, None, "founder").expect("second");

    assert!(!again.changed);
    assert_eq!(again.grants, 1);
    assert!(again.audit_record.is_none());
    assert_eq!(
        audit_records(&root).len(),
        1,
        "a no-op writes no second audit record"
    );
}

/// Revoking removes one actor from every grant naming them, and leaves the
/// other actors on a shared grant working.
#[test]
fn revoking_takes_one_actor_out_without_disturbing_the_others() {
    let root = store();
    let types = ["agent.lesson.v1".to_string()];
    grant(&root, "worker", "agent.memory", &types, None, "founder").expect("worker");
    grant(&root, "other", "agent.memory", &types, None, "founder").expect("other");

    let report = revoke_grant(&root, "worker", Some("done"), "founder").expect("revoke");
    let after = config::load(&root).expect("load");

    assert!(report.changed);
    assert_eq!(report.grants, 1);
    assert!(
        identity::require_type_writer(&after, "worker", "agent.memory", "agent.lesson.v1").is_err()
    );
    identity::require_type_writer(&after, "other", "agent.memory", "agent.lesson.v1")
        .expect("the other actor still writes");
}

#[test]
fn revoking_an_actor_that_holds_nothing_is_a_no_op() {
    let root = store();

    let report = revoke_grant(&root, "stranger", None, "founder").expect("revoke");

    assert!(!report.changed);
    assert!(report.audit_record.is_none());
    assert!(audit_records(&root).is_empty());
}

#[test]
fn only_the_root_owner_may_write_grants() {
    let root = store();
    let types = ["agent.lesson.v1".to_string()];

    for actor in ["legacy", "worker"] {
        assert!(matches!(
            grant(&root, "worker", "agent.memory", &types, None, actor),
            Err(Error::PermissionDenied)
        ));
        assert!(matches!(
            revoke_grant(&root, "worker", None, actor),
            Err(Error::PermissionDenied)
        ));
    }
    assert!(show(&root).expect("show").grants.is_empty());
}

#[test]
fn a_grant_needs_a_valid_actor_and_at_least_one_type() {
    let root = store();

    assert!(matches!(
        grant(
            &root,
            " ",
            "agent.memory",
            &["a.v1".into()],
            None,
            "founder"
        ),
        Err(Error::InvalidActor)
    ));
    assert!(matches!(
        grant(&root, "worker", "agent.memory", &[], None, "founder"),
        Err(Error::InvalidType(_))
    ));
    assert!(show(&root).expect("show").grants.is_empty());
}

/// The read surface reports the authority in force, which is what a caller
/// checks before deciding whether a handover is still needed.
#[test]
fn the_read_surface_reports_owner_writers_and_grants_together() {
    let root = store();
    grant(
        &root,
        "worker",
        "agent.memory",
        &["agent.lesson.v1".into()],
        None,
        "founder",
    )
    .expect("grant");

    let report = show(&root).expect("show");

    assert_eq!(report.owner, "founder");
    assert_eq!(report.writers, vec!["founder", "legacy"]);
    assert_eq!(report.grants.len(), 1);
    assert_eq!(report.grants[0].actors, vec!["worker".to_string()]);
    assert_eq!(report.grants[0].namespace, "agent.memory");
}
