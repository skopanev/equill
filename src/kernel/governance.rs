use crate::kernel::error::Error;
use crate::kernel::identity;
use crate::kernel::lock::StoreLock;
use crate::kernel::store::{self, StoreConfig};
use std::fs;
use std::path::Path;

const LOCK: &str = "governance.lock";

/// Proof that the holder is the root owner, and that the store cannot change
/// hands while they act.
///
/// Every root-governed operation used to read authority and then take its own
/// lock, which left a window: a handover landing in between meant the operation
/// ran with an authority the store no longer recognised. Holding one lock across
/// both the check and the work closes it, and makes governance operations
/// serialize against each other rather than interleave.
///
/// The guard deliberately does not nest. An operation that already holds it must
/// reach the already-authorized entry point of whatever it calls, not the public
/// one — a second acquisition would deadlock, which is a louder failure than a
/// silent authority gap and is caught by the first test that exercises the path.
pub struct RootGuard {
    _lock: StoreLock,
}

impl RootGuard {
    /// Take the governance lock, then decide whether this actor governs. The
    /// order matters: reading authority first would answer from a store that
    /// could change before the answer is used.
    pub fn acquire(store_root: &Path, actor: &str) -> Result<(Self, StoreConfig), Error> {
        fs::create_dir_all(store_root.join("locks"))?;
        let lock = StoreLock::named(store_root, LOCK)?;
        let config = store::load(store_root)?;
        identity::require_root(&config, actor)?;
        Ok((Self { _lock: lock }, config))
    }

    /// Take the lock without deciding anything. Used by the recovery entry
    /// point, which has to settle a pending transaction before the question of
    /// who governs can even be answered.
    pub fn unchecked(store_root: &Path) -> Result<Self, Error> {
        fs::create_dir_all(store_root.join("locks"))?;
        Ok(Self {
            _lock: StoreLock::named(store_root, LOCK)?,
        })
    }

    /// Re-read authority while still holding the lock. Recovery can change who
    /// the owner is, so a guard taken before it has to ask again after.
    pub fn reauthorize(&self, store_root: &Path, actor: &str) -> Result<StoreConfig, Error> {
        let config = store::load(store_root)?;
        identity::require_root(&config, actor)?;
        Ok(config)
    }
}
