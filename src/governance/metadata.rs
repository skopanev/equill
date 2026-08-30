use crate::kernel::digest::sha256_hex;
use crate::kernel::error::Error;
use crate::kernel::lock::StoreLock;
use crate::kernel::store::{self, StoreConfig};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// The metadata a mutation intends to write, hashed before anything is
/// committed. Knowing the resulting digest in advance is what lets the audit
/// record state both ends of the change while still being written first.
pub(super) struct Plan {
    pub(super) bytes: Vec<u8>,
    pub(super) before: String,
    pub(super) after: String,
}

/// A store.json rewrite that is never observable half-done. The file is
/// validated with `deny_unknown_fields`, so a partial write is not a degraded
/// store — it is one that no longer loads at all.
struct Staged {
    temporary: PathBuf,
    final_path: PathBuf,
}

impl Staged {
    fn commit(mut self) -> Result<(), Error> {
        fs::rename(&self.temporary, &self.final_path)?;
        self.temporary = PathBuf::new();
        if let Some(directory) = self.final_path.parent() {
            File::open(directory).and_then(|file| file.sync_all())?;
        }
        Ok(())
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        if !self.temporary.as_os_str().is_empty() {
            let _ = fs::remove_file(&self.temporary);
        }
    }
}

pub(super) fn digest(store_root: &Path) -> Result<String, Error> {
    Ok(sha256_hex(&fs::read(store_root.join("store.json"))?))
}

/// Work out what the change would produce, without writing it. Authorization is
/// re-checked here against the config as it actually is, not as the caller saw
/// it before taking any locks.
pub(super) fn plan<F>(store_root: &Path, expected_owner: &str, change: F) -> Result<Plan, Error>
where
    F: FnOnce(&mut StoreConfig) -> Result<(), Error>,
{
    let _lock = StoreLock::exclusive(store_root)?;
    let before = digest(store_root)?;
    let mut config = store::load(store_root)?;
    if config.root_owner != expected_owner {
        return Err(Error::PermissionDenied);
    }
    change(&mut config)?;
    // Validate the RESULT, not the request. A change that produces metadata the
    // store cannot load would otherwise commit successfully and brick the store
    // on the next open — the write reports success, and everything after it
    // fails.
    store::validate(&config)?;
    let bytes = serde_json::to_vec_pretty(&config)?;
    let after = sha256_hex(&bytes);
    Ok(Plan {
        bytes,
        before,
        after,
    })
}

/// Commit a plan, or refuse. This is a compare-and-swap on the whole file: the
/// full digest is re-read under the lock and must still equal the one the plan
/// was built from. Anything else means another change landed in between, and
/// writing now would silently discard it.
pub(super) fn commit(store_root: &Path, plan: &Plan) -> Result<(), Error> {
    let _lock = StoreLock::exclusive(store_root)?;
    if digest(store_root)? != plan.before {
        return Err(Error::StoreMismatch);
    }
    write_bytes(store_root, &plan.bytes)
}

/// The same swap, from bytes recovery already holds. Recovery has verified the
/// digest itself and must write exactly what the audit record attested, not a
/// re-derived serialization of it.
pub(super) fn write_bytes(store_root: &Path, bytes: &[u8]) -> Result<(), Error> {
    let temporary = store_root.join(format!(".store-{}.json", Uuid::now_v7()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(&temporary);
        return Err(Error::Io(error));
    }
    Staged {
        temporary,
        final_path: store_root.join("store.json"),
    }
    .commit()
}
