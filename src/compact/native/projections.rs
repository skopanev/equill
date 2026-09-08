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
use super::plan::Plan;
use crate::kernel::error::Error;
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
    if let Some(projection) = crate::vector::VectorProjection::open(store_root)? {
        drop_points(&projection, condemned)?;
    }
    crate::projection::rebuild(store_root)?;
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

/// Where the condemned ids wait between the swap and a successful
/// reconciliation.
///
/// Without this, a compaction that swapped the ledger and then failed — or
/// died — has lost the only list of what to remove: the ledger no longer names
/// those records, so a second run plans nothing and exits, leaving the points
/// behind forever. The file is written before the swap and deleted only once
/// the projections agree, so it is a step in a transaction rather than a
/// history of the dead.
const PENDING: &str = "compact-pending.json";

pub fn stash(store_root: &std::path::Path, condemned: &[Uuid]) -> Result<(), Error> {
    if condemned.is_empty() {
        return Ok(());
    }
    let path = store_root.join(PENDING);
    std::fs::write(&path, serde_json::to_vec(condemned)?)?;
    Ok(())
}

/// What an interrupted compaction left to finish, if anything.
pub fn unfinished(store_root: &std::path::Path) -> Result<Vec<Uuid>, Error> {
    let path = store_root.join(PENDING);
    match std::fs::read(&path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.into()),
    }
}

pub fn settled(store_root: &std::path::Path) {
    let _ = std::fs::remove_file(store_root.join(PENDING));
}
