mod boundaries;
mod grants;
mod integrity;
mod serialization;
mod stale_root;
pub(crate) mod transfer;
mod unaffected;
mod wildcards;

use crate::command::init;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);

/// A store with an owner, one legacy writer beside them, and a namespace to
/// grant against. Synthetic throughout: no real identity, path or payload.
pub(super) fn store() -> PathBuf {
    let suffix = NEXT.fetch_add(1, Ordering::Relaxed);
    let root =
        std::env::temp_dir().join(format!("equill-governance-{}-{suffix}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    init::create_with_writers(
        &root,
        "founder",
        "agent.memory",
        &["founder".into(), "legacy".into()],
    )
    .expect("initialize");
    root
}

/// Drive a transfer up to the point the metadata swap would happen, then stop:
/// journal written, audit appended, store.json untouched. Returns the
/// transaction id the journal is holding.
pub(super) fn interrupt(root: &std::path::Path, new_owner: &str) -> uuid::Uuid {
    use super::{audit, journal, metadata};
    let owner = crate::kernel::store::load(root).expect("load").root_owner;
    audit::prepare(root, &owner).expect("prepare");
    let plan = metadata::plan(root, &owner, |config| {
        config.root_owner = new_owner.into();
        config.writers.retain(|item| item != &owner);
        Ok(())
    })
    .expect("plan");
    let tx_id = uuid::Uuid::now_v7();
    journal::write(
        root,
        &journal::Pending {
            tx_id,
            action: "owner-transfer".into(),
            subject: new_owner.into(),
            before_sha256: plan.before.clone(),
            after_sha256: plan.after.clone(),
            after_bytes: String::from_utf8(plan.bytes.clone()).expect("utf-8"),
        },
    )
    .expect("journal");
    audit::record(
        root,
        &owner,
        tx_id,
        "owner-transfer",
        new_owner,
        (&plan.before, &plan.after),
        None,
    )
    .expect("audit");
    tx_id
}

pub(super) fn journal_exists(root: &std::path::Path) -> bool {
    super::journal::path(root).is_file()
}
