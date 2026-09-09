//! What the ledger holds, counted the way the rest of the store counts it.
//!
//! The numbers come from the same rules the lifecycle and the corpus already
//! use — a record is superseded when something names it, withdrawn when
//! `record::withdrawn` says so. A second definition of "dead" living here would
//! drift from the one that decides what a search returns, and the status would
//! start describing a store nobody else sees.
use crate::kernel::error::Error;
use crate::record::StoredRecord;
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct LedgerCounts {
    /// Every row in the ledger, including the ones nothing serves any more.
    pub ledger_records: usize,
    pub ledger_live: usize,
    /// Superseded and revoked counted together, each record once.
    pub ledger_dead: usize,
    pub ledger_superseded: usize,
    pub ledger_revoked: usize,
    /// Records that are both, which is why the two above can sum to more than
    /// `ledger_dead`. Not what a tombstone does to the claim it withdraws — the
    /// claim carries no tag and the tombstone replaces nothing else. It is a
    /// tombstone that was itself later replaced: A, then a tombstone naming A,
    /// then a record naming the tombstone.
    pub ledger_dead_overlap: usize,
}

pub fn ledger(store_root: &Path) -> Result<LedgerCounts, Error> {
    Ok(of(&crate::record::read_all(store_root)?))
}

pub fn of(records: &[StoredRecord]) -> LedgerCounts {
    let replaced = records
        .iter()
        .filter_map(|record| record.supersedes)
        .collect::<HashSet<_>>();
    let (mut superseded, mut revoked, mut overlap) = (0, 0, 0);
    for record in records {
        let is_superseded = replaced.contains(&record.id);
        let is_revoked = crate::record::withdrawn(record);
        superseded += usize::from(is_superseded);
        revoked += usize::from(is_revoked);
        overlap += usize::from(is_superseded && is_revoked);
    }
    let dead = superseded + revoked - overlap;
    LedgerCounts {
        ledger_records: records.len(),
        ledger_live: records.len() - dead,
        ledger_dead: dead,
        ledger_superseded: superseded,
        ledger_revoked: revoked,
        ledger_dead_overlap: overlap,
    }
}
