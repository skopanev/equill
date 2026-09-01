//! Naming an actor that may read this store and may not change it.
//!
//! Every other authority here grants, and a wildcard grants to everyone. This
//! one refuses, which is why it needs saying separately: a store that has
//! opened itself to `*` cannot take one actor back out by editing a list of
//! names it does not contain.
use super::report::ReaderReport;
use super::{apply, authorize, metadata};
use crate::kernel::error::Error;
use crate::kernel::identity;
use crate::kernel::store::{self, StoreConfig};
use std::path::Path;

/// Say that an actor may read and not write.
///
/// Never the root owner. Governance is what lifts this restriction, and the
/// root owner is who governs; marking them would leave a store no command
/// could recover — the one actor able to undo it would be the one refused.
pub fn deny(
    store_root: &Path,
    subject: &str,
    comment: Option<&str>,
    actor: &str,
) -> Result<ReaderReport, Error> {
    let (guard, config) = authorize(store_root, actor)?;
    // Named for the argument that is wrong, not for the environment variable:
    // `InvalidActor` reads as "EQUILL_ACTOR is not a stable identity", which
    // sends the reader to look at the wrong thing entirely.
    if !identity::valid(subject) {
        return Err(Error::Governance(format!(
            "--actor {subject:?} is not a stable identity"
        )));
    }
    if subject == "*" {
        // `*` is a valid identity everywhere it grants, and would be a
        // catastrophe here: it reads as "hold everyone to reading", which
        // includes the owner, and nothing could then undo it. Refused where it
        // is written rather than honoured and regretted — and refused loudly,
        // because accepting it and then matching nobody would record a policy
        // the store does not enforce.
        return Err(Error::Governance(
            "--actor * would hold every actor to reading, including the owner, and nothing could undo it; name actors explicitly".into(),
        ));
    }
    if subject == config.root_owner {
        return Err(Error::Governance(format!(
            "{subject} owns this store: governance is what lifts a read-only hold, so holding the owner would leave nobody able to lift it"
        )));
    }
    if config.read_only.iter().any(|item| item == subject) {
        return report(store_root, subject, false, None);
    }
    let named = subject.to_owned();
    apply(
        &guard,
        store_root,
        actor,
        "reader-add",
        subject,
        comment,
        move |config: &mut StoreConfig| {
            config.read_only.push(named);
            config.read_only.sort();
            Ok(())
        },
    )
    .and_then(|(record, _)| report(store_root, subject, true, Some(record)))
}

/// Let an actor write again, as far as everything else allows.
///
/// Removing the refusal does not grant anything: what the actor may do
/// afterwards is whatever `writers` and the grants already said.
pub fn allow(
    store_root: &Path,
    subject: &str,
    comment: Option<&str>,
    actor: &str,
) -> Result<ReaderReport, Error> {
    let (guard, config) = authorize(store_root, actor)?;
    if !identity::valid(subject) {
        return Err(Error::Governance(format!(
            "--actor {subject:?} is not a stable identity"
        )));
    }
    if !config.read_only.iter().any(|item| item == subject) {
        return report(store_root, subject, false, None);
    }
    let named = subject.to_owned();
    apply(
        &guard,
        store_root,
        actor,
        "reader-revoke",
        subject,
        comment,
        move |config: &mut StoreConfig| {
            config.read_only.retain(|item| item != &named);
            Ok(())
        },
    )
    .and_then(|(record, _)| report(store_root, subject, true, Some(record)))
}

fn report(
    store_root: &Path,
    subject: &str,
    changed: bool,
    audit_record: Option<uuid::Uuid>,
) -> Result<ReaderReport, Error> {
    Ok(ReaderReport {
        ok: true,
        actor: subject.to_owned(),
        readers: store::load(store_root)?.read_only.len(),
        changed,
        audit_record,
        store_sha256: metadata::digest(store_root)?,
    })
}
