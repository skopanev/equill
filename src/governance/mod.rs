mod audit;
mod grant;
mod journal;
mod metadata;
mod owner;
mod reader;
mod recover;
mod report;

use crate::kernel::error::Error;
use crate::kernel::governance::RootGuard;
use crate::kernel::lock::StoreLock;
use crate::kernel::store::{self, StoreConfig};
use std::path::Path;
use uuid::Uuid;

pub use audit::{TYPE as AUDIT_TYPE, TYPE_V2 as AUDIT_TYPE_V2};
pub use grant::{grant, revoke_grant};
pub use owner::transfer;
pub use reader::{allow as allow_writes, deny as deny_writes};
pub use report::{AuthorityReport, GrantReport, GrantView, OwnerReport, ReaderReport};

/// A read of the authority in force. Taking the writer lock keeps this from
/// reporting a store.json that a concurrent handover is in the middle of
/// replacing.
pub fn show(store_root: &Path) -> Result<AuthorityReport, Error> {
    let _lock = StoreLock::exclusive(store_root)?;
    let config = store::load(store_root)?;
    Ok(AuthorityReport {
        ok: true,
        owner: config.root_owner,
        writers: config.writers,
        read_only: {
            let mut held = config.read_only.clone();
            held.sort();
            held
        },
        grants: config
            .write_grants
            .into_iter()
            .map(|grant| GrantView {
                actors: grant.actors,
                namespace: grant.namespace,
                types: grant.types,
            })
            .collect(),
        store_sha256: metadata::digest(store_root)?,
    })
}

/// Take the governance guard and settle the store under it, then decide whether
/// this actor governs.
///
/// The guard is held from here until the caller drops it, so nothing else can
/// change hands or govern in between. Recovery runs first because a store with a
/// transaction in flight has an owner that is still being decided — and it can
/// change that owner, which is why authority is read AFTER it rather than
/// before.
pub(super) fn authorize(store_root: &Path, actor: &str) -> Result<(RootGuard, StoreConfig), Error> {
    let guard = RootGuard::unchecked(store_root)?;
    recover::run(store_root)?;
    let config = guard.reauthorize(store_root, actor)?;
    Ok((guard, config))
}

/// One governance transaction, start to finish.
///
/// The transaction lock is held across the whole thing, so two governance calls
/// serialize even though each takes and releases the writer lock several times
/// inside. Any interrupted transaction is finished or abandoned before a new one
/// is allowed to begin — a store never carries two.
///
/// The journal is written before the audit, and the audit before the metadata,
/// because a transfer strips the old owner of both root and writer access:
/// commit the metadata first and the only identity that could still explain what
/// happened no longer has permission to write it down.
pub(super) fn apply<F>(
    _guard: &RootGuard,
    store_root: &Path,
    actor: &str,
    action: &str,
    subject: &str,
    comment: Option<&str>,
    change: F,
) -> Result<(Uuid, String), Error>
where
    F: FnOnce(&mut StoreConfig) -> Result<(), Error>,
{
    audit::prepare(store_root, actor)?;
    let plan = metadata::plan(store_root, actor, change)?;
    let tx_id = Uuid::now_v7();
    journal::write(
        store_root,
        &journal::Pending {
            tx_id,
            action: action.to_owned(),
            subject: subject.to_owned(),
            before_sha256: plan.before.clone(),
            after_sha256: plan.after.clone(),
            after_bytes: String::from_utf8(plan.bytes.clone())
                .map_err(|_| Error::Integrity("store metadata is not valid utf-8".into()))?,
        },
    )?;
    let record = audit::record(
        store_root,
        actor,
        tx_id,
        action,
        subject,
        (&plan.before, &plan.after),
        comment,
    )?;
    metadata::commit(store_root, &plan)?;
    journal::clear(store_root)?;
    Ok((record, plan.after.clone()))
}

/// Finish or abandon an interrupted governance transaction. Exposed so a store
/// can be settled without changing anything else about it; every mutation does
/// this for itself under the guard it already holds.
pub fn recover(store_root: &Path) -> Result<Option<String>, Error> {
    let _guard = RootGuard::unchecked(store_root)?;
    recover::run(store_root)
}

#[cfg(test)]
mod tests;
