//! What a native store would remove, and what would change in what stays.
//!
//! The manifest path rebuilds a store from the inputs that produced it. A store
//! written through `record` has no such inputs — the ledger is the source — so
//! the plan is computed from the ledger itself and applied to it in place.
use crate::kernel::error::Error;
use crate::record::StoredRecord;
use serde::Serialize;
use std::collections::HashSet;
use uuid::Uuid;

/// Why a record is not carried forward.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Removal {
    /// A later record supersedes it.
    Superseded,
    /// It was withdrawn, and the tombstone goes with it.
    Revoked,
}

#[derive(Debug, Serialize)]
pub struct Removed {
    pub id: Uuid,
    #[serde(rename = "type")]
    pub type_name: String,
    pub reason: Removal,
}

/// A record that stays, but not unchanged: its `supersedes` pointed at
/// something that is being removed.
#[derive(Debug, Serialize)]
pub struct Severed {
    pub id: Uuid,
    #[serde(rename = "type")]
    pub type_name: String,
    /// Named so the report can say what the link used to reach. The ancestor
    /// itself is gone; this is the report, not a record kept elsewhere.
    pub was: Uuid,
}

#[derive(Debug, Default, Serialize)]
pub struct Plan {
    pub removed: Vec<Removed>,
    pub severed: Vec<Severed>,
    pub retained: usize,
}

/// A tombstone carries the revoked tag; both it and what it withdrew go.
fn withdrawn(record: &StoredRecord) -> bool {
    record
        .tags
        .iter()
        .any(|tag| tag == crate::record::REVOKED_TAG)
}

pub fn build(records: &[StoredRecord]) -> Result<Plan, Error> {
    let replaced = records
        .iter()
        .filter_map(|record| record.supersedes)
        .collect::<HashSet<_>>();
    let dead = records
        .iter()
        .filter(|record| replaced.contains(&record.id) || withdrawn(record))
        .map(|record| record.id)
        .collect::<HashSet<_>>();

    let mut plan = Plan::default();
    for record in records {
        if dead.contains(&record.id) {
            plan.removed.push(Removed {
                id: record.id,
                type_name: record.type_name.clone(),
                reason: if withdrawn(record) {
                    Removal::Revoked
                } else {
                    Removal::Superseded
                },
            });
            continue;
        }
        plan.retained += 1;
        // A live record whose ancestor is going: the link would dangle, and
        // lifecycle validation refuses a target it cannot find. The edge is
        // cut, which changes this record's envelope and its hash — said plainly
        // rather than described as leaving the record untouched.
        if let Some(was) = record.supersedes.filter(|id| dead.contains(id)) {
            plan.severed.push(Severed {
                id: record.id,
                type_name: record.type_name.clone(),
                was,
            });
        }
    }
    Ok(plan)
}
