//! Native compaction end to end: plan, stage, swap, reconcile.
use super::{apply, journal, projections};
use crate::kernel::error::Error;
use crate::kernel::governance::RootGuard;
use serde::Serialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct NativeReport {
    pub ok: bool,
    pub applied: bool,
    pub removed: usize,
    pub severed: usize,
    pub retained: usize,
    /// Named plainly: these records survive with a different envelope hash
    /// than they had, because the link into the removed past was cut.
    pub rewritten_envelopes: usize,
    pub detail: projections::Plan,
}

/// Finishes any interrupted compaction, then compacts. Two operations with
/// their own locks: recovery releases the writer lock before reconciling,
/// which the rebuild needs.
pub fn run(store_root: &Path, apply_changes: bool, actor: &str) -> Result<NativeReport, Error> {
    if !apply_changes {
        // A dry run changes nothing, including finishing somebody else's
        // transaction: the caller asked what would happen, not for the store
        // to be altered.
        if journal::Journal::read(store_root)?.is_some() {
            return Err(Error::Compact(
                "an interrupted compaction is still pending; run with --apply to finish it".into(),
            ));
        }
        return compact_once(store_root, false, actor);
    }
    super::recover::recover_previous(store_root, actor)?;
    compact_once(store_root, apply_changes, actor)
}

fn compact_once(
    store_root: &Path,
    apply_changes: bool,
    actor: &str,
) -> Result<NativeReport, Error> {
    let (_guard, _config) = RootGuard::acquire(store_root, actor)?;
    // Governance and the writer hold different locks on purpose, so holding the
    // governance one says nothing about appends. Reading the ledger without the
    // writer's lock means a record written between here and the swap would be
    // dropped by the rewrite that never saw it.
    let writer = crate::kernel::lock::StoreLock::exclusive(store_root)?;
    // Before the ledger is read, because an interruption inside a rename can
    // leave the ledger directory missing entirely — and then reading it first
    // means the store can never repair itself.
    let records = crate::record::read_all(store_root)?;
    // The window a concurrent append falls into. A test needs it wide enough to
    // aim at: guessing at timing is how a race test comes out green whether or
    // not the lock is held.
    #[cfg(test)]
    pause_after_read();
    let plan = projections::build(&records)?;
    let report = NativeReport {
        ok: true,
        applied: apply_changes,
        removed: plan.removed.len(),
        severed: plan.severed.len(),
        retained: plan.retained,
        rewritten_envelopes: plan.severed.len(),
        detail: plan,
    };
    if !apply_changes {
        return Ok(report);
    }
    if report.removed == 0 {
        // Nothing to do, and saying so without touching the store is what makes
        // a second run a no-op rather than a rewrite that happens to match.
        return Ok(report);
    }
    // Taken before the ledger stops naming them.
    let condemned = projections::condemned(&report.detail);
    let transaction = uuid::Uuid::now_v7().simple().to_string();
    let shadow = super::super::transaction::sibling(store_root, "native", &transaction)?;
    // Staged in full first: a journal pointing at an incomplete copy would send
    // recovery into a state that was never prepared.
    stage(store_root, &shadow, &records, &report.detail)?;
    let mut source = std::collections::BTreeMap::new();
    for relative in journal::STEPS {
        source.extend(journal::measure(store_root, relative)?);
    }
    let mut journal = journal::Journal {
        transaction: transaction.clone(),
        shadow: shadow.clone(),
        phase: journal::Phase::Staged,
        condemned: condemned.clone(),
        source,
    };
    journal.write(store_root)?;
    publish(store_root, &shadow, &transaction, &mut journal)?;
    // Released before reconciling: rebuilding the text projection takes the
    // same writer lock, and holding it here would deadlock against ourselves.
    // An append landing in this window is fine — it is in the ledger, and the
    // rebuild that follows reads the ledger.
    drop(writer);
    projections::reconcile(store_root, &condemned)?;
    // Only after the projections agree. Until then the journal is what makes
    // the work findable again.
    cleanup(store_root, &shadow, &transaction)?;
    journal::Journal::clear(store_root)?;
    Ok(report)
}

/// The whole new state is built beside the store first. Nothing the reader can
/// see changes until the swap.
fn stage(
    store_root: &Path,
    shadow: &Path,
    records: &[crate::record::StoredRecord],
    plan: &projections::Plan,
) -> Result<(), Error> {
    fs::create_dir_all(shadow)?;
    let kept = apply::rewrite(records, plan);
    apply::stage_records(shadow, &kept)?;
    copy_tree(
        &store_root.join("receipts/writes"),
        &shadow.join("receipts/writes"),
    )?;
    apply::drop_receipts(shadow, plan)?;
    apply::reconcile_receipts(shadow, &kept)?;
    Ok(())
}

/// Backups and the staged copy go once the projections agree: until then they
/// are what makes an interrupted compaction recoverable.
pub(super) fn cleanup(store_root: &Path, shadow: &Path, transaction: &str) -> Result<(), Error> {
    for relative in journal::STEPS {
        let current = store_root.join(relative);
        let backup = super::super::transaction::sibling(&current, "backup", transaction)?;
        apply::remove(&backup)?;
    }
    apply::remove(shadow)
}

/// Publishes each prepared directory and records how far it got. The phase is
/// written after the rename it describes, so it always lags by one window,
/// which is why recovery reads the directories rather than the phase.
fn publish(
    store_root: &Path,
    shadow: &Path,
    transaction: &str,
    journal: &mut journal::Journal,
) -> Result<(), Error> {
    for (index, relative) in journal::STEPS.iter().enumerate() {
        let current = store_root.join(relative);
        let incoming = shadow.join(relative);
        let backup = super::super::transaction::sibling(&current, "backup", transaction)?;
        #[cfg(test)]
        journal::halt_if_asked(&format!("kill-before-{relative}"));
        #[cfg(test)]
        journal::interrupt_at(&format!("before-{relative}"))?;
        // The gap inside the pair: old directory aside, new one not yet there.
        #[cfg(test)]
        if journal::interrupting(&format!("inside-{relative}")) {
            std::fs::rename(&current, &backup)?;
            journal::sync_directory(store_root)?;
            return Err(Error::Compact("interrupted inside the rename".into()));
        }
        #[cfg(test)]
        if journal::asked_to_halt(&format!("kill-inside-{relative}")) {
            // Leave the gap a crash would leave, then die without unwinding.
            std::fs::rename(&current, &backup)?;
            journal::sync_directory(store_root)?;
            std::process::abort();
        }
        super::super::transaction::swap(&current, &incoming, &backup)?;
        if let Some(parent) = current.parent() {
            journal::sync_directory(parent)?;
        }
        // Between the rename and the phase write: the directory has moved and
        // the journal has not caught up.
        #[cfg(test)]
        journal::interrupt_at(&format!("after-{relative}"))?;
        journal.advance(
            store_root,
            if index == 0 {
                journal::Phase::RecordsSwapped
            } else {
                journal::Phase::ReceiptsSwapped
            },
        )?;
    }
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), Error> {
    if !from.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let path = entry?.path();
        let target = to.join(path.file_name().unwrap_or_default());
        if path.is_dir() {
            copy_tree(&path, &target)?;
        } else {
            fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

// How long to linger between reading the ledger and publishing, for tests that
// need a concurrent write to land in the middle. Zero everywhere else, and
// compiled out of a release build entirely.
#[cfg(test)]
thread_local! {
    static PAUSE: std::cell::Cell<std::time::Duration> =
        const { std::cell::Cell::new(std::time::Duration::ZERO) };
}

#[cfg(test)]
fn pause_after_read() {
    let waiting = PAUSE.with(std::cell::Cell::get);
    if !waiting.is_zero() {
        std::thread::sleep(waiting);
    }
}

#[cfg(test)]
pub(crate) fn with_pause<T>(waiting: std::time::Duration, body: impl FnOnce() -> T) -> T {
    struct Restore(std::time::Duration);
    impl Drop for Restore {
        fn drop(&mut self) {
            PAUSE.with(|slot| slot.set(self.0));
        }
    }
    let _restore = Restore(PAUSE.with(std::cell::Cell::get));
    PAUSE.with(|slot| slot.set(waiting));
    body()
}
