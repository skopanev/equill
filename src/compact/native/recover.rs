//! Finishing a compaction that was interrupted.
//!
//! Always forward, never back. A rollback would be correct only if nothing had
//! happened since the crash, and something has: the store accepts writes again
//! as soon as the process is gone, so undoing a published ledger would erase
//! records appended after it. What was prepared is completed instead.
//!
//! The journal says what was being attempted; the directories say how far it
//! got. The phase alone cannot answer that, because it is written after the
//! rename it describes — so between the two there is always a moment where the
//! directory has moved and the journal has not.
use super::journal;
use super::journal::{Journal, STEPS, sync_directory};
use super::projections;
use crate::kernel::error::Error;
use crate::kernel::governance::RootGuard;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

/// What one directory's publication looks like on disk.
enum Step {
    /// Prepared, not yet published.
    Pending,
    /// Published; nothing left to do.
    Done,
    /// The old directory was moved aside and the new one never arrived. This is
    /// the gap inside a rename pair, and the only state where the store is
    /// missing a directory it needs.
    Interrupted,
    /// Neither the prepared copy nor the current directory is there, or both
    /// claim to be authoritative. Not something to guess at.
    Unclear,
}

fn inspect(
    store_root: &Path,
    shadow: &Path,
    relative: &str,
    transaction: &str,
) -> Result<Step, Error> {
    let current = store_root.join(relative);
    let incoming = shadow.join(relative);
    let backup = super::super::transaction::sibling(&current, "backup", transaction)?;
    Ok(
        match (current.is_dir(), incoming.is_dir(), backup.is_dir()) {
            // Prepared and not yet published, with nothing set aside.
            (true, true, false) => Step::Pending,
            // A backup beside both: an earlier attempt got further than this
            // combination admits, and picking which of the two is
            // authoritative is how a store loses a directory.
            (true, true, true) => Step::Unclear,
            (true, false, _) => Step::Done,
            // The prepared copy is there and the live directory is not: finish
            // the arrival.
            (false, true, _) => Step::Interrupted,
            // Only the backup survives, so the rename that moved it aside never
            // completed. Restoring it leaves a readable store — and the work
            // unfinished, which the journal still records.
            (false, false, true) => Step::Interrupted,
            (false, false, false) => Step::Unclear,
        },
    )
}

/// Completes an interrupted publication and reconciles what follows it.
pub fn resume(store_root: &Path, journal: Journal) -> Result<(), Error> {
    let shadow = journal.shadow.clone();
    for relative in STEPS {
        match inspect(store_root, &shadow, relative, &journal.transaction)? {
            Step::Done => {}
            Step::Pending => {
                // The store has been writable since the crash, and an append
                // lands in the directory that is still current. Publishing the
                // prepared copy over it would carry that record away in the
                // backup — lost from an immutable ledger. The prepared copy is
                // only good if nothing has been added to what it replaces.
                if changed_since_staging(store_root, relative, &journal.source)? {
                    return Err(Error::Compact(format!(
                        "{relative} changed after the interrupted compaction staged it; \
                         resolve by hand rather than lose the newer writes"
                    )));
                }
                publish_step(store_root, &shadow, relative, &journal.transaction)?;
            }
            Step::Interrupted => {
                publish_step(store_root, &shadow, relative, &journal.transaction)?;
            }
            Step::Unclear => {
                return Err(Error::Compact(format!(
                    "compaction {} left {relative} in a state that cannot be resumed safely",
                    journal.transaction
                )));
            }
        }
    }
    Ok(())
}

/// Whether the live directory has gained anything since the staged copy was
/// built. An append adds a record to a month file, so the staged copy either
/// lacks that file or holds fewer bytes of it.
fn changed_since_staging(
    store_root: &Path,
    relative: &str,
    source: &std::collections::BTreeMap<String, u64>,
) -> Result<bool, Error> {
    let now = super::journal::measure(store_root, relative)?;
    let then: std::collections::BTreeMap<_, _> = source
        .iter()
        .filter(|(name, _)| name.starts_with(relative))
        .collect();
    if now.len() != then.len() {
        return Ok(true);
    }
    Ok(now
        .iter()
        .any(|(name, size)| then.get(name).is_none_or(|before| *before != size)))
}

/// Moves the prepared directory into place, or restores the backup when the
/// prepared one is gone. Nothing is deleted here: deleting is what a rollback
/// does, and this is not one.
fn publish_step(
    store_root: &Path,
    shadow: &Path,
    relative: &str,
    transaction: &str,
) -> Result<(), Error> {
    let current = store_root.join(relative);
    let incoming = shadow.join(relative);
    let backup = super::super::transaction::sibling(&current, "backup", transaction)?;
    if current.is_dir() && incoming.is_dir() {
        fs::rename(&current, &backup)?;
    }
    if incoming.is_dir() {
        fs::rename(&incoming, &current)?;
    } else if !current.is_dir() && backup.is_dir() {
        // The old directory was moved aside and the new one never arrived: put
        // the old one back, so the store is readable and the next run can plan
        // the work again from a ledger that exists.
        fs::rename(&backup, &current)?;
    }
    if let Some(parent) = current.parent() {
        sync_directory(parent)?;
    }
    Ok(())
}

/// Completes an interrupted publication and reconciles what it left, if any.
pub(super) fn recover_previous(store_root: &Path, actor: &str) -> Result<(), Error> {
    // Held for the whole recovery, not just the directory work: two
    // compactions racing for one journal would each believe they own it.
    let (_guard, _config) = RootGuard::acquire(store_root, actor)?;
    let recovered = {
        // Under the writer lock and before anything is read: an interruption
        // inside a rename can leave the ledger directory missing, and reading
        // first would mean the store can never repair itself.
        // The writer lock is released here and not before reconciling, because
        // the rebuild takes it.
        let _writer = crate::kernel::lock::StoreLock::exclusive(store_root)?;
        finish_previous(store_root)?
    };
    let Some(recovered) = recovered else {
        return Ok(());
    };
    projections::reconcile(store_root, &recovered.condemned)?;
    super::run::cleanup(store_root, &recovered.shadow, &recovered.transaction)?;
    journal::Journal::clear(store_root)
}

/// Completes an interrupted publication, before this run reads anything: an
/// interruption inside a rename can leave the ledger directory missing, and
/// then reading first means the store can never repair itself.
///
/// Returns what still has to be dropped from the projections. That part waits
/// until the writer lock is released.
struct Recovered {
    condemned: Vec<uuid::Uuid>,
    shadow: PathBuf,
    transaction: String,
}

fn finish_previous(store_root: &Path) -> Result<Option<Recovered>, Error> {
    let Some(journal) = journal::Journal::read(store_root)? else {
        return Ok(None);
    };
    let recovered = Recovered {
        condemned: journal.condemned.clone(),
        shadow: journal.shadow.clone(),
        transaction: journal.transaction.clone(),
    };
    resume(store_root, journal)?;
    Ok(Some(recovered))
}
