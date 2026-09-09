use super::ModelArtifact;
use crate::kernel::error::Error;
use crate::vector::model::vector_error;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs::File;
use std::io::Read;
use std::path::Path;

pub(super) fn verify_artifact(
    store: &Path,
    artifact: &ModelArtifact,
    role: &str,
) -> Result<(), Error> {
    let path = if artifact.path.is_absolute() {
        artifact.path.clone()
    } else {
        store.join(&artifact.path)
    };
    let mut file =
        File::open(path).map_err(|_| vector_error(&format!("{role} artifact missing")))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| vector_error(&format!("{role} artifact unreadable")))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let mut actual = String::with_capacity(64);
    for byte in hasher.finalize() {
        write!(&mut actual, "{byte:02x}").expect("writing to String cannot fail");
    }
    if actual != artifact.sha256 {
        return Err(vector_error(&format!("{role} artifact hash mismatch")));
    }
    Ok(())
}
