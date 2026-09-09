use super::{PENDING, complete, invalid, load, record, storage, verify_receipt};
use crate::kernel::{error::Error, path};
use std::{
    fs::{self, File},
    path::Path,
};

/// Under the writer lock, before receipt settlement and before another append.
pub(crate) fn recover(root: &Path) -> Result<(), Error> {
    let directory = path::within(root, PENDING)?;
    if !directory.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(directory)? {
        let name = path::plain_name(&entry?.path())?;
        if storage::is_temporary(&name) {
            storage::remove(root, &format!("{PENDING}/{name}"))?;
            continue;
        }
        let digest = name
            .strip_suffix(".json")
            .ok_or_else(|| invalid("pending"))?;
        let relative = format!("{PENDING}/{name}");
        let outcome = load(root, &relative, digest)?;
        if record(root, &outcome)?.is_some() {
            // Complete cached bytes are not proof the interrupted writer
            // reached sync_data. Make truth durable before attesting to it.
            File::open(path::file_within(root, &outcome.ledger)?)?.sync_all()?;
            File::open(path::within(root, "records")?)?.sync_all()?;
            crate::record::receipt::resolve_pending(root)?;
            verify_receipt(root, &outcome)?;
            complete(root, &outcome)?;
        } else {
            storage::remove(root, &relative)?;
        }
    }
    Ok(())
}
