use super::desired;
use super::worker::{self, DrainReport};
use crate::kernel::error::Error;
use crate::kernel::lock::StoreLock;
use std::path::Path;

/// Record what the ledger now holds, then get the index moving toward it
/// without making the caller wait.
///
/// Called only after a durable commit. Nothing here can unmake a record: an
/// unreachable provider leaves the write successful, the previous checkpoint
/// serving, and a sanitized note in the report.
/// `appended` is how many records the caller just made durable — supplied by the
/// canonical append path, which is the only thing that knows. Zero means "start
/// a worker if one is needed" without moving the target, which is what a caller
/// that wrote nothing must do: a revision that counted calls rather than records
/// would drift away from the ledger.
pub fn after_commit(store: &Path, appended: u64) -> DrainReport {
    match publish_then_start(store, appended) {
        Ok(mut report) => {
            // A write that just advanced the target cannot be current: the
            // record it added is by definition not indexed yet. Saying so
            // directly avoids re-reading the markers to rediscover it.
            report.projection = if appended > 0 {
                match enabled(store) {
                    Ok(true) => super::worker::Projection::Queued,
                    _ => super::worker::Projection::Disabled,
                }
            } else {
                projection(store)
            };
            report.settle()
        }
        Err(error) => DrainReport {
            attempt_error: Some(error.to_string()),
            ..DrainReport::default()
        },
    }
}

/// Get a store moving again without writing to it.
///
/// This is what makes crash recovery automatic rather than manual: a worker
/// killed mid-pass leaves its lock released by the operating system and its
/// target still on disk, so the next ordinary command — a search, a read,
/// anything that opens the store — finds work outstanding and starts it.
pub fn resume(store: &Path) -> DrainReport {
    if !outstanding(store) {
        return DrainReport::default();
    }
    start(store)
}

/// Whether this store owes the index anything, answered from local files only.
///
/// Every command calls this, so it must be cheap and it must be quiet: a store
/// that is already current has to spawn nothing, or an ordinary read on a
/// healthy store would start a process it has no use for.
#[cfg(test)]
pub(crate) fn outstanding_for_tests(store: &Path) -> bool {
    outstanding(store)
}

fn outstanding(store: &Path) -> bool {
    let Ok(config) = super::super::config::load(store) else {
        // Unreadable config: this gate is not the place to diagnose it, and a
        // store nobody can configure is not one to start work on.
        return false;
    };
    let Some(config) = config.filter(|config| config.enabled) else {
        return false;
    };
    let Ok(Some(target)) = desired::read(store) else {
        // No target published means nothing has been asked for yet.
        return false;
    };
    // Two small files, compared directly. Asking `freshness` would re-read and
    // re-hash the entire ledger, which is fine for a report and unacceptable
    // for a gate that runs before every command.
    match super::super::state::checkpoint(store, Some(&config)) {
        // The worker records the revision it actually covered, so a write during
        // a pass leaves the numbers apart and forces another one.
        Some(covered) => covered != target.revision,
        // No usable checkpoint against a published target: the index cannot be
        // shown to be current, so treat it as work to do.
        None => true,
    }
}

/// Publish the target and do the catch-up in this process.
///
/// The synchronous path, kept for callers that want the write to block until the
/// index agrees — and used by the tests that are about catch-up semantics rather
/// than about the handoff, so they do not depend on a process-wide environment
/// variable while running in parallel.
pub fn after_commit_inline(store: &Path, appended: u64) -> DrainReport {
    match publish(store, appended) {
        // No ticket here: an inline caller has already been authorized to write,
        // and it is doing the work in its own process rather than reaching the
        // worker command. The ticket exists to gate that command.
        Ok(true) => {
            let mut report = worker::run_once(store);
            report.projection = projection(store);
            report.settle()
        }
        Ok(false) => DrainReport::default(),
        Err(error) => DrainReport {
            attempt_error: Some(error.to_string()),
            ..DrainReport::default()
        },
    }
}

/// Record that the ledger has moved, without reading it.
///
/// The old form hashed the whole corpus here, which put a linear scan on the
/// path of every append. The target is now a counter the writer bumps; the
/// worker does the enumeration and hashing on its own time.
fn publish(store: &Path, appended: u64) -> Result<bool, Error> {
    if !enabled(store)? {
        // No vector projection to advance — but the text index still has to
        // catch up, and that is the worker's job too. Returning early here is
        // what left a store with the projection off with nothing to make it
        // searchable.
        return Ok(true);
    }
    if appended == 0 {
        // Nothing new to want. The caller still gets a worker started if one is
        // outstanding, but the target must not move for a call that wrote
        // nothing.
        return Ok(true);
    }
    let _writers = StoreLock::exclusive(store)?;
    desired::advance(store, appended)?;
    Ok(true)
}

fn publish_then_start(store: &Path, appended: u64) -> Result<DrainReport, Error> {
    // The config is read locally, before anything could touch the network, so a
    // store with the projection off never opens a connection at all.
    //
    // The snapshot and its publication happen under the writer lock. Reading the
    // ledger without it let two writers interleave: one captured N, the other
    // committed and published N+1, and the first then overwrote the watermark
    // back to N — after which the drain compared an index at N+1 against a
    // target of N and never agreed with itself.
    if !publish(store, appended)? {
        return Ok(DrainReport::default());
    }
    Ok(start(store))
}

/// Start a worker unless one is already running.
///
/// The probe is the whole coalescing story: a burst publishes many revisions and
/// starts one worker, because every writer after the first finds this lock held.
/// That is safe precisely because the holder re-reads the target on each pass and
/// checks equality under the writer lock, so it cannot leave while a published
/// revision is unindexed.
/// Make sure one worker is running, and start one if not.
///
/// The claim lock is what makes this atomic. Probing the drain lock and then
/// releasing it before spawning is a race: two writers can both see it free and
/// each start a child. Deciding under the claim, and holding the claim until the
/// child has actually taken the drain lock, means exactly one child exists per
/// gap — which is the whole of the coalescing guarantee.
/// Make sure one worker is running, and start one if not.
///
/// No lock probing and no waiting. Creating the claim is itself atomic, so the
/// caller either created it — and is therefore the one starting a worker — or it
/// did not, and somebody else already is. An append never waits to find out
/// which: waiting for a child to prove ownership put the provider's latency back
/// on the writing path, which is the whole thing this release removes.
/// Where the index stands, for a caller that has just written. Answered from
/// the same two markers the gate uses, so it costs nothing extra.
/// The projection state a record just written is in: queued if there is an
/// index at all, because the record cannot already be in it.
pub fn projection_after_write(store: &Path) -> super::worker::Projection {
    match enabled(store) {
        Ok(true) => super::worker::Projection::Queued,
        _ => super::worker::Projection::Disabled,
    }
}

pub fn projection(store: &Path) -> super::worker::Projection {
    match enabled(store) {
        Ok(true) if outstanding(store) => super::worker::Projection::Queued,
        Ok(true) => super::worker::Projection::Current,
        _ => super::worker::Projection::Disabled,
    }
}

fn start(store: &Path) -> DrainReport {
    if super::cooldown::in_effect(store) {
        // A recent attempt for this same target, model and config failed. Trying
        // again now would spawn a doomed child and charge every writer for it.
        // The target is already published, so the next eligible activity picks
        // the work up.
        return DrainReport::default();
    }
    let Ok(Some(_)) = super::handoff::claim(store) else {
        return DrainReport::default();
    };
    if super::starter::starter()(store).is_ok() {
        DrainReport {
            spawned: true,
            ..DrainReport::default()
        }
    } else {
        // Nothing was started, so the claim must not linger: one refused spawn
        // would otherwise stop every later writer until it went stale.
        super::handoff::release(store);
        DrainReport::default()
    }
}

fn enabled(store: &Path) -> Result<bool, Error> {
    Ok(super::super::config::load(store)?
        .filter(|config| config.enabled)
        .is_some())
}
