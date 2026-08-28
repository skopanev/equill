use crate::kernel::error::Error;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize, Serialize)]
pub struct StoreConfig {
    pub format_version: u64,
    pub root_owner: String,
    pub namespaces: Vec<String>,
    /// Actors allowed to append records besides the root owner. Empty keeps the
    /// store owner-only; `["*"]` opens it to every agent on the machine.
    #[serde(default)]
    pub writers: Vec<String>,
    pub created_at_unix_ms: u128,
}

pub fn load(root: &Path) -> Result<StoreConfig, Error> {
    let path = root.join("store.json");
    if !path.is_file() {
        return Err(Error::NotInitialized(root.to_path_buf()));
    }
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
