use super::super::operator;
use super::desired;
use crate::kernel::error::Error;
use crate::kernel::lock::{StoreLock, TryLock};
use serde::Serialize;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

pub(super) const LOCK: &str = "vector-drain.lock";

/// Bounds that keep a worker finite no matter what the ledger or the provider
/// does. Neither is a tuning knob for throughput — they exist so that a process
/// which cannot make progress cannot keep running either.
const MAX_PASSES: usize = 64;
const DEADLINE: Duration = Duration::from_secs(900);

/// Where the projection stands after a write, as distinct from whether the
/// write itself is safe.
#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Projection {
    /// The index already reflects this write.
    Current,
    /// The write is durable and the index will follow. This is the ordinary
    /// answer, and it is not a failure.
    #[default]
    Queued,
    /// The store does not have the projection turned on.
    Disabled,
    /// There is no record to index: the write was refused before it reached the
    /// ledger, so no projection state describes it.
    #[serde(rename = "not-applicable")]
    NotApplicable,
}

#[derive(Debug, Default, Serialize)]
pub struct DrainReport {
    /// Whether this process did the catching up. A caller that handed off to a
    /// detached worker did not.
    pub ran: bool,
    /// Whether a detached worker was started for this revision. Reporting a
    /// handoff is not what makes a write successful — the ledger is.
    #[serde(skip_serializing_if = "is_false")]
    pub spawned: bool,
    /// What a caller should tell its user about search freshness.
    pub projection: Projection,
    pub passes: usize,
    pub embeddings: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt_error: Option<String>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl DrainReport {
    /// Enforce the one combination that must never be published: an attempt
    /// that failed alongside an index reported as current. Together they tell a
    /// caller the index is up to date AND that bringing it up to date just
    /// failed, which cannot both be true and is exactly the shape 0.2.9 shipped.
    pub(super) fn settle(mut self) -> Self {
        if self.attempt_error.is_some() && matches!(self.projection, Projection::Current) {
            self.projection = Projection::Queued;
        }
        self
    }
}

/// Catch the index up to whatever the ledger holds now, then stop.
///
/// Every pass re-reads the target, so a worker started for one revision settles
/// the latest one instead — which is how a burst of writes is absorbed by a
/// single process. It exits at equality, exits on a provider error, and exits
/// when either bound is reached. There is no path through this function that
/// waits, retries or sleeps, so it cannot become a resident service.
/// A worker's last outcome, kept on disk so a detached run is not invisible.
///
/// Written atomically — staged, fsynced, renamed, directory fsynced — so a
/// reader never sees half a report and a crash never leaves a stale temp. But
/// the WRITE ITSELF is best effort: failing to file a report must never turn a
/// successful catch-up into a failed one, so every error here is swallowed. It
/// is a report about work, not the work.
///
/// Sanitized on purpose: counts, an outcome word, and an error CLASS. No
/// payloads, no record identifiers, no provider messages that might quote one.
#[derive(Debug, Serialize)]
struct LastDrain<'a> {
    schema: &'a str,
    outcome: &'a str,
    passes: usize,
    embeddings: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_class: Option<&'a str>,
}

const LAST_DRAIN: &str = "projections/qdrant/last-drain.json";

/// Which kind of failure this was, without repeating what the provider said.
/// A provider can echo arbitrary text, and a durable file is exactly the wrong
/// place to discover that it echoed a payload.
fn error_class(error: &str) -> &'static str {
    if error.contains("not configured") {
        "not-configured"
    } else if error.contains("artifact") || error.contains("model") {
        "model-unavailable"
    } else if error.contains("bound") {
        "stopped-at-bound"
    } else {
        "provider-unavailable"
    }
}

fn record_outcome(store: &Path, report: &DrainReport) {
    let outcome = if report.attempt_error.is_some() {
        // Remember the failure so the next fifty writers do not each start a
        // child that will fail the same way.
        super::cooldown::record_failure(store);
        "failed"
    } else {
        // Converged: whatever was wrong is not wrong any more.
        super::cooldown::clear(store);
        "converged"
    };
    let state = LastDrain {
        schema: "equill.qdrant-last-drain.v1",
        outcome,
        passes: report.passes,
        embeddings: report.embeddings,
        error_class: report.attempt_error.as_deref().map(error_class),
    };
    let Ok(bytes) = serde_json::to_vec(&state) else {
        return;
    };
    let path = store.join(LAST_DRAIN);
    let Some(directory) = path.parent() else {
        return;
    };
    if fs::create_dir_all(directory).is_err() {
        return;
    }
    let temporary = directory.join(format!(".last-drain-{}.json", uuid::Uuid::now_v7()));
    let staged = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .and_then(|mut file| {
            file.write_all(&bytes)?;
            file.sync_all()
        });
    if staged.is_err() {
        let _ = fs::remove_file(&temporary);
        return;
    }
    if fs::rename(&temporary, &path).is_err() {
        let _ = fs::remove_file(&temporary);
        return;
    }
    let _ = File::open(directory).and_then(|handle| handle.sync_all());
}

/// The worker entry point a started child reaches.
///
/// Consuming the handoff is the authorization boundary, and it belongs here
/// rather than inside the loop: it must fail the COMMAND, not be folded into a
/// report that the shell reads as success. Hiding the subcommand from `--help`
/// is presentation; this is the part that refuses.
pub fn run_worker(store: &Path) -> Result<DrainReport, Error> {
    // Ownership is held for the whole run and released on the way out, so a
    // writer arriving mid-run sees that somebody is already on it.
    let _ownership = super::handoff::consume(store)?;
    Ok(run_once(store))
}

pub fn run_once(store: &Path) -> DrainReport {
    let Ok(Some(lease)) = TryLock::acquire(store, LOCK) else {
        // Somebody is already draining, or the lock is unusable. Either way this
        // process has nothing to do and must not wait.
        return DrainReport::default();
    };
    let started = Instant::now();
    let mut report = DrainReport {
        ran: true,
        ..DrainReport::default()
    };
    loop {
        match operator::catch_up(store) {
            Ok(pass) => {
                report.passes += 1;
                report.embeddings += pass.embeddings;
            }
            Err(error) => {
                // A provider that is down does not cost us the record. The next
                // write, or the next command against this store, tries again.
                report.attempt_error = Some(error.to_string());
                record_outcome(store, &report);
                return report;
            }
        }
        // The equality check happens while the writer lock is held and the drain
        // lock is released inside it, so a writer that has already committed is
        // visible here and one that has not yet committed will find this lock
        // free once it does. No tail is ever both unseen and unclaimed.
        let Ok(writers) = StoreLock::exclusive(store) else {
            return report;
        };
        match caught_up(store) {
            Ok(true) => {
                lease.release();
                drop(writers);
                record_outcome(store, &report);
                return report;
            }
            Ok(false) => drop(writers),
            Err(error) => {
                drop(writers);
                report.attempt_error = Some(error.to_string());
                record_outcome(store, &report);
                return report;
            }
        }
        if report.passes >= MAX_PASSES || started.elapsed() >= DEADLINE {
            report.attempt_error = Some("drain stopped at its bound without converging".into());
            record_outcome(store, &report);
            return report;
        }
    }
}

pub(crate) fn caught_up(store: &Path) -> Result<bool, Error> {
    let Some(target) = desired::read(store)? else {
        return Ok(true);
    };
    let config = super::super::config::load(store)?;
    let reading = super::super::report::freshness(store, config.as_ref())?;
    // The revision the last pass covered has to equal what is wanted now, and
    // the index has to actually be current. Either one alone can be true while
    // work remains.
    let covered = super::super::state::checkpoint(store, config.as_ref());
    Ok(covered == Some(target.revision)
        && reading.freshness == super::super::VectorFreshness::Current)
}
