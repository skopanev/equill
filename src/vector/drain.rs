use super::{desired, operator};
use crate::kernel::error::Error;
use crate::kernel::lock::{StoreLock, TryLock};
use serde::Serialize;
use std::path::Path;

const LOCK: &str = "vector-drain.lock";

#[derive(Debug, Default, Serialize)]
pub struct DrainReport {
    /// Whether this process did the catching up, or found somebody already at
    /// it. Both are successful outcomes; only one of them does work.
    pub ran: bool,
    pub passes: usize,
    pub embeddings: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt_error: Option<String>,
}

/// Publishes what the ledger now holds, then catches the index up in this very
/// process — no daemon, no detached child, nothing that outlives the command.
///
/// A writer that finds somebody already draining does not wait and does not
/// spawn: it has already recorded what it wants indexed, and the holder is
/// obliged to see that tail before it leaves.
///
/// Called only after a durable commit. Nothing here can unmake a record: an
/// unreachable provider leaves the write successful, the previous checkpoint
/// serving, and a sanitized note in the report.
pub fn after_commit(store: &Path, actor: &str) -> DrainReport {
    match publish_and_drain(store, actor) {
        Ok(report) => report,
        Err(error) => DrainReport {
            ran: false,
            passes: 0,
            embeddings: 0,
            attempt_error: Some(error.to_string()),
        },
    }
}

fn publish_and_drain(store: &Path, actor: &str) -> Result<DrainReport, Error> {
    // The config is read locally, before anything could touch the network, so a
    // store with the projection off never opens a connection at all.
    if super::config::load(store)?
        .filter(|config| config.enabled)
        .is_none()
    {
        return Ok(idle());
    }
    // The snapshot and its publication happen under the writer lock. Reading
    // the ledger without it let two writers interleave: one captured N, the
    // other committed and published N+1, and the first then overwrote the
    // watermark back to N — after which the drain compared an index at N+1
    // against a target of N and never agreed with itself.
    //
    // The lock is released before the drain lock is taken, so the order between
    // the two is always writer-then-drain and no writer ever waits on an
    // embedding run.
    {
        let _writers = StoreLock::exclusive(store)?;
        let (records, digest) = operator::corpus(store)?;
        desired::publish(store, records.len(), &digest)?;
    }
    let Some(lease) = TryLock::acquire(store, LOCK)? else {
        // Somebody is draining. The watermark is already published, so their
        // final check will see this tail.
        return Ok(idle());
    };
    let mut report = DrainReport {
        ran: true,
        passes: 0,
        embeddings: 0,
        attempt_error: None,
    };
    loop {
        match operator::sync(store, actor) {
            Ok(pass) => {
                report.passes += 1;
                report.embeddings += pass.embeddings;
            }
            Err(error) => {
                // A provider that is down does not cost us the record, and we
                // do not retry in the background: the next write or an explicit
                // sync tries again.
                report.attempt_error = Some(error.to_string());
                return Ok(report);
            }
        }
        // The final comparison happens while the writer lock is held and the
        // drain lock is released inside it. A writer that has already committed
        // is therefore visible here; one that has not yet committed will find
        // this lock free once it does. There is no gap where a tail is both
        // unseen and unclaimed.
        let writers = StoreLock::exclusive(store)?;
        if caught_up(store)? {
            lease.release();
            drop(writers);
            return Ok(report);
        }
        drop(writers);
    }
}

pub(super) fn caught_up(store: &Path) -> Result<bool, Error> {
    let Some(target) = desired::read(store)? else {
        return Ok(true);
    };
    let config = super::config::load(store)?;
    let reading = super::state::freshness(store, config.as_ref())?;
    Ok(reading.indexed_records == Some(target.records)
        && reading.freshness == super::VectorFreshness::Current)
}

fn idle() -> DrainReport {
    DrainReport {
        ran: false,
        passes: 0,
        embeddings: 0,
        attempt_error: None,
    }
}
