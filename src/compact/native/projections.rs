//! Taking the removed records out of the projections too.
//!
//! This has to happen as part of compaction, not after it. The vector sync
//! learns which points are stale by reading the ledger for superseded and
//! revoked records — and once compaction has removed those lines, there is
//! nothing left to learn from. A point whose record is gone from the ledger is
//! never revisited: it would sit in the collection, answering searches, with no
//! record behind it.
//!
//! So the list of ids is taken while the ledger still names them, and spent
//! before it stops.
use crate::kernel::error::Error;
use crate::record::StoredRecord;
use serde::Serialize;
use std::collections::HashSet;
use uuid::Uuid;

/// The ids the plan is about to make unknowable.
pub fn condemned(plan: &Plan) -> Vec<Uuid> {
    plan.removed.iter().map(|item| item.id).collect()
}

/// Drops the points and rebuilds the text index.
///
/// The text projection is derived from the ledger and is rebuilt wholesale,
/// which is cheap and exact. The vector projection is not rebuilt: its points
/// are expensive, the surviving ones are still correct, and only the condemned
/// are removed.
pub fn reconcile(store_root: &std::path::Path, condemned: &[Uuid]) -> Result<(), Error> {
    // Held while the points are removed, and released before the catch-up that
    // needs it. A sync running alongside has already read a snapshot from
    // before the compaction: it would upsert points for records this call is
    // deleting, and they would come back with nothing in the ledger behind
    // them. Taking the lease means either it finished before this started or
    // it has not started yet.
    let removed = {
        let lease = crate::kernel::lock::TryLock::acquire(store_root, "vector-drain.lock")?;
        if lease.is_none() {
            return Err(Error::Compact(
                "a catch-up is running; compaction cannot remove points it might re-add".into(),
            ));
        }
        forget_condemned(store_root, condemned)?
    };
    let configured = removed;
    crate::projection::rebuild(store_root)?;
    // The survivors whose links were cut have new record hashes, and their
    // points still carry the old ones. Left to the next ordinary write, the
    // index would disagree with the ledger until something unrelated happened
    // to touch it — so the catch-up runs here, in the operation that caused the
    // disagreement.
    //
    // It costs no embeddings: the embedding input carries no provenance, so a
    // cut link changes the record hash and not the text. Those points are
    // relabelled rather than recomputed, and the catch-up settles the cursor
    // and the watermark on the way.
    let caught_up = crate::vector::after_commit_inline(store_root, 0);
    if let Some(error) = caught_up.attempt_error {
        return Err(Error::Compact(format!(
            "compaction could not bring the vector projection up to date: {error}"
        )));
    }
    // Absence of an error is not the same as having done the work. A catch-up
    // that could not take the drain lease returns a default report — no error,
    // nothing done — and reading that as success would let compaction declare
    // the projections settled, clear its journal, and leave the survivors
    // carrying stale hashes with nothing left to notice.
    // Only where there is a projection to settle. A store that never asked for
    // a vector has nothing to catch up, and that also reports `ran: false`.
    if configured && !caught_up.ran {
        return Err(Error::Compact(
            "another catch-up holds the vector lease; compaction cannot confirm the \
             projection is settled"
                .into(),
        ));
    }
    Ok(())
}

/// Anything that can forget a point. Named as a trait so the decision — which
/// ids go, and that they go at all — can be tested without a live provider,
/// which is the only part of this a test could otherwise not see.
pub trait Forgetful {
    fn active(&self) -> Result<String, Error>;
    fn forget(&self, physical: &str, ids: &[Uuid]) -> Result<(), Error>;
}

impl Forgetful for crate::vector::VectorProjection {
    fn active(&self) -> Result<String, Error> {
        self.active_collection()
    }

    fn forget(&self, physical: &str, ids: &[Uuid]) -> Result<(), Error> {
        self.delete(physical, ids)
    }
}

/// Removes the condemned points, and only those.
///
/// A failure here is a failure of the compaction. It cannot be swallowed and
/// left for the next sync: a sync learns which points are stale by reading the
/// ledger for superseded and revoked records, and after compaction those lines
/// are gone — so nothing will ever name these points again. A store whose
/// provider was unreachable would keep them forever while the command that
/// removed their records reported success.
pub fn drop_points(index: &impl Forgetful, condemned: &[Uuid]) -> Result<(), Error> {
    if condemned.is_empty() {
        return Ok(());
    }
    let physical = index.active()?;
    index.forget(&physical, condemned)
}

/// Drops the condemned points through whatever index this store has, and says
/// whether there was one.
///
/// A seam because the only honest test of "compaction removed the points and
/// recomputed nothing" needs an index that answers: a real provider is
/// unreachable in a test, and an unreachable one fails for a reason that has
/// nothing to do with what is being measured.
fn forget_condemned(store_root: &std::path::Path, condemned: &[Uuid]) -> Result<bool, Error> {
    #[cfg(test)]
    if let Some(outcome) = substitute(condemned) {
        return outcome;
    }
    match crate::vector::VectorProjection::open(store_root)? {
        Some(projection) => {
            drop_points(&projection, condemned)?;
            Ok(true)
        }
        None => Ok(false),
    }
}

#[cfg(test)]
type Substitute = std::sync::Arc<dyn Fn(&[Uuid]) -> Result<bool, Error> + Send + Sync>;

#[cfg(test)]
thread_local! {
    static INDEX: std::cell::RefCell<Option<Substitute>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn substitute(condemned: &[Uuid]) -> Option<Result<bool, Error>> {
    INDEX
        .with(|slot| slot.borrow().clone())
        .map(|index| index(condemned))
}

/// Installs a stand-in index for one call, and takes it away afterwards even if
/// the body panics.
#[cfg(test)]
pub(super) fn with_index<T>(index: Substitute, body: impl FnOnce() -> T) -> T {
    struct Restore(Option<Substitute>);
    impl Drop for Restore {
        fn drop(&mut self) {
            INDEX.with(|slot| *slot.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(INDEX.with(|slot| slot.borrow_mut().take()));
    INDEX.with(|slot| *slot.borrow_mut() = Some(index));
    body()
}

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
