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
/// Shared by the registered-profile path and the native contract so that the
/// two produce the same receipt, the same digest and the same selection —
/// which is what lets a caller compare them at all, and what keeps "one call"
/// from meaning "a second way of answering".
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
    // Filing the receipt is a side effect of answering, not part of it: the
    // whole receipt is in the answer either way, and a store that cannot be
    // written to can still be read. What that does NOT cover is a receipt
    // whose bytes disagree with one already filed under the same digest —
    // that is not "could not write", it is the store contradicting itself,
    // and swallowing it would turn an integrity fault into a quiet field.
    let receipt_path = match persist(store_root, &receipt) {
        Ok(path) => Some(path),
        Err(error @ Error::Integrity(_)) => return Err(error),
        Err(_) => None,
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
