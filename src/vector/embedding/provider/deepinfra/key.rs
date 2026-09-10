//! Reading the credential without reading whatever else the path points at.
use super::super::super::super::model::vector_error;
use crate::kernel::error::Error;
use std::fs;

/// The largest thing that can sensibly be an API key. Anything past this is a
/// pointed-at-the-wrong-file mistake, and reading it is the harm.
const MAX_KEY_BYTES: usize = 8192;

pub(super) fn read_bounded(path: &std::path::Path) -> Result<String, Error> {
    use std::io::Read;
    let mut file = fs::File::open(path)
        .map_err(|_| vector_error("deepinfra API key file could not be read"))?;
    let mut buffer = Vec::new();
    file.by_ref()
        .take(MAX_KEY_BYTES as u64 + 1)
        .read_to_end(&mut buffer)
        .map_err(|_| vector_error("deepinfra API key file could not be read"))?;
    if buffer.len() > MAX_KEY_BYTES {
        return Err(vector_error(
            "deepinfra API key file is too large to be a key",
        ));
    }
    String::from_utf8(buffer).map_err(|_| vector_error("deepinfra API key file is not text"))
}
