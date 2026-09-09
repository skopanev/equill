//! What the plan says goes, what stays, and what stays changed.
use super::projections::{Forgetful, condemned, drop_points};
use super::{Removal, build};
use crate::command::init;
use crate::kernel::error::Error;
use crate::record::StoredRecord;
use crate::record::{RecordDraft, append};
use crate::schema::{self, TypeDefinition};
use serde_json::json;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use uuid::Uuid;

fn record(id: Uuid, supersedes: Option<Uuid>, tags: Vec<String>) -> StoredRecord {
    StoredRecord {
        id,
        namespace: "agent.memory".into(),
        type_name: "agent.lesson.v1".into(),
        actor: "owner".into(),
        recorded_at: "2026-01-01T00:00:00Z".into(),
        observed_at: "2026-01-01T00:00:00Z".into(),
        valid_at: "2026-01-01T00:00:00Z".into(),
        payload: json!({ "rule": "a rule" }),
        evidence: Vec::new(),
        tags,
        supersedes,
    }
}

/// A chain keeps only its living end, and the link that reached into the
/// removed past is reported as cut — not quietly left dangling, and not
/// described as an untouched record.
#[test]
fn a_chain_collapses_to_its_living_end_and_the_link_is_cut() {
    let (first, second, third) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let plan = build(&[
        record(first, None, Vec::new()),
        record(second, Some(first), Vec::new()),
        record(third, Some(second), Vec::new()),
    ])
    .expect("plan");

    assert_eq!(plan.retained, 1);
    assert_eq!(plan.removed.len(), 2);
    assert!(
        plan.removed
            .iter()
            .all(|item| item.reason == Removal::Superseded)
    );
    assert_eq!(plan.severed.len(), 1, "the surviving link was not reported");
    assert_eq!(plan.severed[0].id, third);
    assert_eq!(plan.severed[0].was, second);
}

/// A withdrawal takes both the claim and the tombstone: keeping either would
/// leave the retraction half-done.
#[test]
fn a_revocation_removes_the_claim_and_its_tombstone() {
    let (claim, tombstone) = (Uuid::now_v7(), Uuid::now_v7());
    let plan = build(&[
        record(claim, None, Vec::new()),
        record(tombstone, Some(claim), vec!["equill:revoked".into()]),
    ])
    .expect("plan");

    assert_eq!(plan.retained, 0, "a withdrawn claim survived");
    assert_eq!(plan.removed.len(), 2);
    assert!(
        plan.removed
            .iter()
            .any(|item| item.reason == Removal::Revoked)
    );
    assert!(plan.severed.is_empty(), "nothing survives to be severed");
}

/// A store with nothing dead in it plans no work, which is what makes a second
/// run after an apply a no-op.
#[test]
fn a_store_of_living_records_plans_nothing() {
    let plan = build(&[
        record(Uuid::now_v7(), None, Vec::new()),
        record(Uuid::now_v7(), None, Vec::new()),
    ])
    .expect("plan");

    assert_eq!(plan.retained, 2);
    assert!(plan.removed.is_empty() && plan.severed.is_empty());
}

/// Every surviving link is cut, and that is not an accident of this fixture.
///
/// A record named by `supersedes` is superseded by definition, so it is always
/// among the removed — there is no such thing as a live record whose ancestor
/// survives. After compaction no retained record carries a `supersedes` at all,
/// and the count of cuts is exactly the count of retained records that had one.
#[test]
fn every_retained_link_is_cut_because_no_ancestor_can_survive() {
    let (first, second, lone) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let records = [
        record(first, None, Vec::new()),
        record(second, Some(first), Vec::new()),
        record(lone, None, Vec::new()),
    ];
    let plan = build(&records).expect("plan");

    let removed: std::collections::HashSet<_> = plan.removed.iter().map(|item| item.id).collect();
    let retained_with_edge = records
        .iter()
        .filter(|item| !removed.contains(&item.id))
        .filter(|item| item.supersedes.is_some())
        .count();

    assert_eq!(
        plan.severed.len(),
        retained_with_edge,
        "the plan and the ledger disagree on how many links survive to be cut"
    );
    assert!(
        !plan.severed.iter().any(|item| item.id == lone),
        "a record that never had a link was reported as cut"
    );
}

#[derive(Default)]
struct Recording {
    forgotten: RefCell<Vec<Uuid>>,
    calls: RefCell<usize>,
}

impl Forgetful for Recording {
    fn active(&self) -> Result<String, Error> {
        Ok("equill_points_1".to_string())
    }

    fn forget(&self, _physical: &str, ids: &[Uuid]) -> Result<(), Error> {
        *self.calls.borrow_mut() += 1;
        self.forgotten.borrow_mut().extend_from_slice(ids);
        Ok(())
    }
}

fn plain(id: Uuid, supersedes: Option<Uuid>) -> StoredRecord {
    StoredRecord {
        id,
        namespace: "agent.memory".into(),
        type_name: "agent.lesson.v1".into(),
        actor: "owner".into(),
        recorded_at: "2026-01-01T00:00:00Z".into(),
        observed_at: "2026-01-01T00:00:00Z".into(),
        valid_at: "2026-01-01T00:00:00Z".into(),
        payload: json!({ "rule": "a rule" }),
        evidence: Vec::new(),
        tags: Vec::new(),
        supersedes,
    }
}

/// The points must be dropped while the ledger still names the records.
///
/// After compaction the ledger no longer lists them, and the sync that would
/// otherwise notice stale points learns which ones are stale by reading the
/// ledger for superseded and revoked records. Removed from the ledger, they
/// become invisible to it — the points would answer searches forever with
/// nothing behind them.
#[test]
fn the_condemned_points_are_dropped_and_the_survivors_are_not() {
    let (first, second, lone) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let records = [
        plain(first, None),
        plain(second, Some(first)),
        plain(lone, None),
    ];
    let plan = build(&records).expect("plan");
    let index = Recording::default();

    drop_points(&index, &condemned(&plan)).expect("drop");

    assert_eq!(
        *index.forgotten.borrow(),
        vec![first],
        "the wrong set of points was removed"
    );
}

/// Nothing to remove means nothing is asked of the provider: a store with a
/// clean ledger should not talk to it at all.
#[test]
fn a_compaction_with_nothing_to_remove_does_not_touch_the_provider() {
    let plan = build(&[plain(Uuid::now_v7(), None)]).expect("plan");
    let index = Recording::default();

    drop_points(&index, &condemned(&plan)).expect("drop");

    assert_eq!(*index.calls.borrow(), 0);
}

pub(super) fn store(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "equill-compact-race-{name}-{}",
        uuid::Uuid::now_v7()
    ));
    init::create(&root, "owner", "agent.memory").expect("init");
    schema::register(
        &root,
        TypeDefinition {
            type_name: "agent.lesson.v1".into(),
            uri: "equill://agent.lesson/v1".into(),
            owner: "owner".into(),
            payload_schema: json!({
                "type": "object",
                "properties": { "rule": { "type": "string" } },
                "required": ["rule"],
                "additionalProperties": false
            }),
            lifecycle: Default::default(),
        },
        "owner",
    )
    .expect("schema");
    root
}

pub(super) fn add(root: &Path, rule: &str, supersedes: Option<uuid::Uuid>) -> uuid::Uuid {
    append(
        root,
        RecordDraft {
            namespace: "agent.memory".into(),
            type_name: "agent.lesson.v1".into(),
            observed_at: "2026-01-01T00:00:00Z".into(),
            valid_at: None,
            payload: json!({ "rule": rule }),
            evidence: Vec::new(),
            tags: Vec::new(),
            supersedes,
        },
        "owner",
    )
    .expect("append")
    .id
}
