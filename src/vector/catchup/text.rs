//! The text-only drain owes the same final tail check as a vector drain.
use super::worker::{DrainReport, record_outcome};
use crate::kernel::lock::{StoreLock, TryLock};
use std::path::Path;
use std::time::Instant;

/// The first text pass has finished while this lease was held. A concurrent
/// append may have coalesced into that pass, so do not leave until its published
/// target is covered. Release the drain lease while holding the writer lock:
/// the next append then either was observed here or can start its own drain.
pub(super) fn finish(store: &Path, lease: TryLock) -> DrainReport {
    let started = Instant::now();
    let (max_passes, deadline) = super::bounds::bounds();
    let mut passes = 1;
    loop {
        #[cfg(test)]
        if let Some(action) = AFTER_PASS.with(std::cell::Cell::get) {
            action(store);
        }
        let Ok(writers) = StoreLock::exclusive(store) else {
            return failed(store, "text drain could not check its target");
        };
        if current(store) {
            lease.release();
            drop(writers);
            let report = DrainReport::default();
            if store.join(super::worker::LAST_DRAIN).exists() {
                record_outcome(store, &report);
            }
            return report;
        }
        drop(writers);
        if passes >= max_passes || started.elapsed() >= deadline {
            return failed(store, "text drain stopped at its bound without converging");
        }
        if crate::projection::catch_up_text_background(store).is_err() {
            return failed(store, "text projection catch-up failed");
        }
        passes += 1;
    }
}

fn current(store: &Path) -> bool {
    let Some(target) = crate::projection::target(store) else {
        // No writer has published a target, so no tracked tail is outstanding.
        return true;
    };
    crate::projection::watermark(store).is_some_and(|covered| {
        covered.indexed_records == target.records && covered.ledger_bytes == target.ledger_bytes
    })
}

fn failed(store: &Path, reason: &str) -> DrainReport {
    let report = DrainReport {
        attempt_error: Some(reason.into()),
        ..DrainReport::default()
    };
    record_outcome(store, &report);
    report
}

#[cfg(test)]
type AfterPass = fn(&Path);
#[cfg(test)]
thread_local! {
    static AFTER_PASS: std::cell::Cell<Option<AfterPass>> = const { std::cell::Cell::new(None) };
}
#[cfg(test)]
pub(crate) fn with_after_pass<T>(action: AfterPass, body: impl FnOnce() -> T) -> T {
    let _restore = super::seam::Restore::install(&AFTER_PASS, action);
    body()
}
