use super::budget::Budgeted;
use super::model::{ContextBudget, ContextBundle, ContextReceipt, VersionCoordinate};
use super::retrieval::Retrieval;
use crate::kernel::digest::sha256_hex;
use crate::kernel::error::Error;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

pub fn persist(store: &Path, receipt: &ContextReceipt) -> Result<String, Error> {
    let mut bytes = serde_json::to_vec(receipt)?;
    bytes.push(b'\n');
    let digest = sha256_hex(&bytes);
    let relative = format!("receipts/context/{digest}.json");
    let path = store.join(&relative);
    if path.exists() {
        if fs::read(&path)? != bytes {
            return Err(Error::Integrity("context receipt hash collision".into()));
        }
        return Ok(relative);
    }
    let directory = store.join("receipts/context");
    fs::create_dir_all(&directory)?;
    let temporary = directory.join(format!(".{digest}.tmp-{}", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    fs::rename(temporary, path)?;
    Ok(relative)
}

/// Turn a finished retrieval into the answer a caller sees.
///
/// Shared by every path that assembles context, so that they produce the same
/// receipt, the same digest and the same selection — which is what lets a
/// caller compare two answers at all.
pub fn bundle(
    store_root: &Path,
    profile: VersionCoordinate,
    selectors: Vec<VersionCoordinate>,
    request_digest: String,
    budget: ContextBudget,
    retrieved: Retrieval,
    budgeted: Budgeted,
) -> Result<ContextBundle, Error> {
    let bundle_digest = sha256_hex(budgeted.content.as_bytes());
    let degraded = budgeted.degraded || !retrieved.degraded_strategies.is_empty();
    let empty = budgeted.selected.is_empty();
    let receipt = ContextReceipt {
        schema: "equill.context-receipt.v1",
        profile,
        selectors,
        request_digest,
        included: budgeted.selected.clone(),
        excluded: budgeted.excluded,
        strategies: retrieved.strategies,
        budget,
        used: budgeted.used,
        bundle_digest: bundle_digest.clone(),
        projection: retrieved.projection,
        degraded_strategies: retrieved.degraded_strategies,
        degraded,
        empty,
        unmatched_coordinates: retrieved.unmatched_coordinates,
    };
    // One failure is tolerated and no others: the store would not take the
    // file. That is what a read-only store looks like, and reading is not a
    // privilege that depends on being able to write.
    //
    // Everything else is fatal. A receipt whose bytes disagree with one already
    // filed under the same digest is the store contradicting itself; a receipt
    // that will not serialize is a bug here. Neither is a store without room,
    // and reporting either as "not persisted" would turn a fault into a field
    // that reads like a disk being full.
    let receipt_path = match persist(store_root, &receipt) {
        Ok(path) => Some(path),
        Err(Error::Io(_)) => None,
        Err(error) => return Err(error),
    };
    let selected_record_ids = receipt.included.iter().map(|item| item.id).collect();
    Ok(ContextBundle {
        ok: true,
        content: budgeted.content,
        bundle_digest,
        selected_record_ids,
        receipt,
        receipt_persisted: receipt_path.is_some(),
        receipt_path,
    })
}
