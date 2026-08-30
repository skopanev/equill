use super::report::GrantReport;
use super::{apply, authorize, metadata};
use crate::kernel::error::Error;
use crate::kernel::identity;
use crate::kernel::store::{self, StoreConfig, WriteGrant};
use std::path::Path;

/// Add a scoped append grant. Least privilege is the point: an actor granted a
/// namespace and a type list can write those and nothing else, and never
/// governs.
pub fn grant(
    store_root: &Path,
    subject: &str,
    namespace: &str,
    types: &[String],
    comment: Option<&str>,
    actor: &str,
) -> Result<GrantReport, Error> {
    let (guard, config) = authorize(store_root, actor)?;
    if !identity::valid(subject) {
        return Err(Error::InvalidActor);
    }
    if types.is_empty() {
        return Err(Error::InvalidType("a grant needs at least one type".into()));
    }
    let wanted = WriteGrant {
        actors: vec![subject.to_owned()],
        namespace: namespace.to_owned(),
        types: types.to_vec(),
    };
    if config.write_grants.iter().any(|grant| same(grant, &wanted)) {
        return Ok(GrantReport {
            ok: true,
            actor: subject.into(),
            grants: config.write_grants.len(),
            changed: false,
            audit_record: None,
            store_sha256: metadata::digest(store_root)?,
        });
    }
    apply(
        &guard,
        store_root,
        actor,
        "grant-add",
        subject,
        comment,
        |config: &mut StoreConfig| {
            config.write_grants.push(WriteGrant {
                actors: wanted.actors.clone(),
                namespace: wanted.namespace.clone(),
                types: wanted.types.clone(),
            });
            Ok(())
        },
    )
    .and_then(|(record, digest)| {
        Ok(GrantReport {
            ok: true,
            actor: subject.into(),
            grants: store::load(store_root)?.write_grants.len(),
            changed: true,
            audit_record: Some(record),
            store_sha256: digest,
        })
    })
}

/// Withdraw every grant naming this actor. Dropping the actor from a shared
/// grant rather than deleting the grant keeps the other actors on it working.
pub fn revoke_grant(
    store_root: &Path,
    subject: &str,
    comment: Option<&str>,
    actor: &str,
) -> Result<GrantReport, Error> {
    let (guard, config) = authorize(store_root, actor)?;
    if !config
        .write_grants
        .iter()
        .any(|grant| grant.actors.iter().any(|item| item == subject))
    {
        return Ok(GrantReport {
            ok: true,
            actor: subject.into(),
            grants: config.write_grants.len(),
            changed: false,
            audit_record: None,
            store_sha256: metadata::digest(store_root)?,
        });
    }
    let (record, digest) = apply(
        &guard,
        store_root,
        actor,
        "grant-revoke",
        subject,
        comment,
        |config: &mut StoreConfig| {
            for grant in &mut config.write_grants {
                grant.actors.retain(|item| item != subject);
            }
            config.write_grants.retain(|grant| !grant.actors.is_empty());
            Ok(())
        },
    )?;
    Ok(GrantReport {
        ok: true,
        actor: subject.into(),
        grants: store::load(store_root)?.write_grants.len(),
        changed: true,
        audit_record: Some(record),
        store_sha256: digest,
    })
}

fn same(left: &WriteGrant, right: &WriteGrant) -> bool {
    left.actors == right.actors && left.namespace == right.namespace && left.types == right.types
}
