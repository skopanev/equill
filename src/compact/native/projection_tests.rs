//! That the removed records are taken out of the vector projection too.
use super::plan::build;
use super::projections::{Forgetful, condemned, drop_points};
use crate::kernel::error::Error;
use crate::record::StoredRecord;
use serde_json::json;
use std::cell::RefCell;
use uuid::Uuid;

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

fn record(id: Uuid, supersedes: Option<Uuid>) -> StoredRecord {
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
        record(first, None),
        record(second, Some(first)),
        record(lone, None),
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
    let plan = build(&[record(Uuid::now_v7(), None)]).expect("plan");
    let index = Recording::default();

    drop_points(&index, &condemned(&plan)).expect("drop");

    assert_eq!(*index.calls.borrow(), 0);
}
