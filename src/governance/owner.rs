use super::report::OwnerReport;
use super::{apply, authorize};
use crate::kernel::error::Error;
use crate::kernel::identity;
use crate::kernel::store::StoreConfig;
use std::path::Path;

/// Hand the store to a new root owner.
///
/// The previous owner loses every form of append with the same call: the
/// store-wide writer entry and any scoped grant naming them. There is no opt-out
/// on purpose — an identity that can still write everything has not handed
/// anything over, and an escape hatch on an authority boundary is the hole the
/// boundary exists to close.
pub fn transfer(
    store_root: &Path,
    new_owner: &str,
    comment: Option<&str>,
    actor: &str,
) -> Result<OwnerReport, Error> {
    let (guard, config) = authorize(store_root, actor)?;
    if !identity::valid(new_owner) || new_owner == config.root_owner {
        return Err(Error::InvalidOwner);
    }
    // A wildcard is an authority this contract cannot take away. `*` means
    // "any valid actor", so the previous owner keeps append access through it
    // no matter what is removed from the lists — and deleting the wildcard
    // outright would silently strip every other actor relying on it. Refuse,
    // name what is in the way, and change nothing.
    if config.writers.iter().any(|writer| writer == "*") {
        return Err(Error::Governance(
            "a store-wide `*` writer would keep the previous owner writable; \
             remove it and name the actors explicitly before handing the store over"
                .into(),
        ));
    }
    if let Some(grant) = config
        .write_grants
        .iter()
        .find(|grant| grant.actors.iter().any(|actor| actor == "*"))
    {
        return Err(Error::Governance(format!(
            "a `*` grant on {} would keep the previous owner writable; \
             name its actors explicitly before handing the store over",
            grant.namespace
        )));
    }
    let previous = config.root_owner.clone();
    let mut revoked = Vec::new();
    if config.writers.iter().any(|item| item == &previous) {
        revoked.push("store-wide append".to_string());
    }
    let scoped = config
        .write_grants
        .iter()
        .filter(|grant| grant.actors.iter().any(|item| item == &previous))
        .count();
    if scoped > 0 {
        revoked.push(format!(
            "{scoped} scoped grant{}",
            if scoped == 1 { "" } else { "s" }
        ));
    }
    let (record, digest) = apply(
        &guard,
        store_root,
        actor,
        "owner-transfer",
        new_owner,
        comment,
        |config: &mut StoreConfig| {
            config.root_owner = new_owner.to_owned();
            config.writers.retain(|item| item != &previous);
            for grant in &mut config.write_grants {
                grant.actors.retain(|item| item != &previous);
            }
            config.write_grants.retain(|grant| !grant.actors.is_empty());
            Ok(())
        },
    )?;
    Ok(OwnerReport {
        ok: true,
        previous_owner: previous,
        owner: new_owner.into(),
        revoked_writers: revoked,
        audit_record: record,
        store_sha256: digest,
    })
}
