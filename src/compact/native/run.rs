//! Native compaction end to end: plan, stage, swap, reconcile.
use super::{apply, plan};
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
    pub detail: plan::Plan,
}

pub fn run(store_root: &Path, apply_changes: bool, actor: &str) -> Result<NativeReport, Error> {
    let (_guard, _config) = RootGuard::acquire(store_root, actor)?;
    let records = crate::record::read_all(store_root)?;
    let plan = plan::build(&records)?;
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
    let transaction = uuid::Uuid::now_v7().simple().to_string();
    let shadow = super::super::transaction::sibling(store_root, "native", &transaction)?;
    stage(store_root, &shadow, &records, &report.detail)?;
    match publish(store_root, &shadow, &transaction) {
        Ok(()) => {
            super::super::transaction::cleanup_tree(&shadow);
            Ok(report)
        }
        Err(error) => {
            super::super::transaction::cleanup_tree(&shadow);
            Err(error)
        }
    }
}

/// The whole new state is built beside the store first. Nothing the reader can
/// see changes until the swap.
fn stage(
    store_root: &Path,
    shadow: &Path,
    records: &[crate::record::StoredRecord],
    plan: &plan::Plan,
) -> Result<(), Error> {
    fs::create_dir_all(shadow)?;
    let kept = apply::rewrite(records, plan);
    apply::stage_records(shadow, &kept)?;
    copy_tree(
        &store_root.join("receipts/writes"),
        &shadow.join("receipts/writes"),
    )?;
    apply::drop_receipts(shadow, plan)?;
    Ok(())
}

/// Swap the directories the compaction rewrote, rolling every one of them back
/// if any fails: a store with a new ledger and old receipts is worse than one
/// that was never compacted.
fn publish(store_root: &Path, shadow: &Path, transaction: &str) -> Result<(), Error> {
    let mut swaps = Vec::new();
    for relative in ["records", "receipts/writes"] {
        let current = store_root.join(relative);
        let incoming = shadow.join(relative);
        let backup = super::super::transaction::sibling(&current, "backup", transaction)?;
        if let Err(error) = super::super::transaction::swap(&current, &incoming, &backup) {
            super::super::transaction::rollback_swaps(&swaps);
            return Err(error);
        }
        swaps.push(super::super::transaction::Swap { current, backup });
    }
    for swap in &swaps {
        super::super::transaction::cleanup_tree(&swap.backup);
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
