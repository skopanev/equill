use super::journal::Pending;
use super::{audit, journal, metadata};
use crate::kernel::digest::sha256_hex;
use crate::kernel::error::Error;
use crate::kernel::lock::StoreLock;
use crate::kernel::store::{self, StoreConfig};
use std::path::Path;

/// Finish or discard an interrupted governance transaction.
///
/// The journal is a convenience copy, not an authority. It sits in a plain file
/// that anything with write access can edit, so nothing in it is acted on until
/// the immutable ledger has confirmed it: the audit record for the transaction
/// is the statement of what was intended, and the bytes are only usable if they
/// hash to what that record attested.
///
/// Every combination is named, including the ones that mean the store is
/// damaged. Those are reported and the journal is kept — discarding the evidence
/// to make an error go away would destroy the only account of what happened.
pub(super) fn run(store_root: &Path) -> Result<Option<String>, Error> {
    let Some(pending) = journal::read(store_root)? else {
        return Ok(None);
    };
    let announced = audit::for_transaction(store_root, pending.tx_id)?;
    if announced.len() > 1 {
        return Err(unrecoverable(&pending, announced.len()));
    }
    // Vet the journal BEFORE choosing a branch, and before anything is cleared.
    //
    // Clearing is itself an action on a tampered journal: it destroys the only
    // evidence that a forgery was attempted. So the bytes are proved against
    // what the ledger attested — or, when nothing was announced, against the
    // journal's own declared digest — and proved to be loadable metadata,
    // before this function decides anything at all.
    let attested = announced
        .first()
        .map(|record| &record["payload"]["store_sha256_after"])
        .and_then(|value| value.as_str());
    vet(&pending, attested)?;
    let live = metadata::digest(store_root)?;

    let Some(record) = announced.first() else {
        // Nothing was announced. The ledger has no claim to act on, so the
        // journal may only be dropped — never applied.
        if live == pending.before_sha256 {
            journal::clear(store_root)?;
            return Ok(Some(format!("abandoned {}", pending.tx_id)));
        }
        return Err(unrecoverable(&pending, 0));
    };
    // The ledger's account of this transaction has to match the journal's in
    // every field. A journal that disagrees with immutable history is not a
    // usable instruction, whatever its digests say.
    let payload = &record["payload"];
    let agrees = payload["action"] == pending.action.as_str()
        && payload["subject"] == pending.subject.as_str()
        && payload["store_sha256_before"] == pending.before_sha256.as_str()
        && payload["store_sha256_after"] == pending.after_sha256.as_str();
    if !agrees {
        return Err(unrecoverable(&pending, 1));
    }
    if live == pending.after_sha256 {
        // Already applied. The journal is cleared only because its bytes have
        // been proved to be the state the store is actually in.
        if sha256_hex(pending.after_bytes.as_bytes()) != live {
            return Err(unrecoverable(&pending, 1));
        }
        journal::clear(store_root)?;
        return Ok(Some(format!("already applied {}", pending.tx_id)));
    }
    if live != pending.before_sha256 {
        return Err(unrecoverable(&pending, 1));
    }
    complete(store_root, &pending)
}

/// Prove the journal's bytes are the bytes they claim to be, and that they are
/// metadata this store could load. Nothing may be applied, and nothing may be
/// cleared, until this passes.
fn vet(pending: &Pending, attested_after: Option<&str>) -> Result<(), Error> {
    let bytes = pending.after_bytes.as_bytes();
    let digest = sha256_hex(bytes);
    // With an audit record, the ledger is the authority on what was intended.
    // Without one, the journal must at least agree with itself.
    let expected = attested_after.unwrap_or(&pending.after_sha256);
    if digest != expected {
        return Err(Error::Integrity(format!(
            "governance transaction {} carries bytes that do not match the digest \
             {} attested",
            pending.tx_id,
            if attested_after.is_some() {
                "the ledger"
            } else {
                "the journal itself"
            }
        )));
    }
    let config: StoreConfig = serde_json::from_slice(bytes)
        .map_err(|_| Error::Integrity("governance journal holds unusable metadata".into()))?;
    store::validate(&config)
}

/// Apply the bytes the audit record attested — after proving they are those
/// bytes, and that they are metadata this store can load.
fn complete(store_root: &Path, pending: &Pending) -> Result<Option<String>, Error> {
    let bytes = pending.after_bytes.as_bytes();
    let lock = StoreLock::exclusive(store_root)?;
    if metadata::digest(store_root)? != pending.before_sha256 {
        return Err(Error::Integrity(
            "governance metadata moved during recovery".into(),
        ));
    }
    metadata::write_bytes(store_root, bytes)?;
    // Confirm the store actually reached the state the ledger says it reached,
    // before the journal that proves what was in flight is thrown away.
    let reached = metadata::digest(store_root)?;
    drop(lock);
    if reached != pending.after_sha256 {
        return Err(Error::Integrity(format!(
            "governance transaction {} did not reach its attested state",
            pending.tx_id
        )));
    }
    journal::clear(store_root)?;
    Ok(Some(format!("completed {}", pending.tx_id)))
}

fn unrecoverable(pending: &Pending, announced: usize) -> Error {
    Error::Integrity(format!(
        "governance transaction {} is unrecoverable: {announced} audit records, \
         and the live metadata is at a digest the ledger does not describe",
        pending.tx_id
    ))
}
