use crate::kernel::error::Error;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize, Serialize)]
pub struct StoreConfig {
    pub format_version: u64,
    pub root_owner: String,
    pub namespaces: Vec<String>,
    pub created_at_unix_ms: u128,
}

pub fn load(root: &Path) -> Result<StoreConfig, Error> {
    let path = root.join("store.json");
    if !path.is_file() {
        return Err(Error::NotInitialized(root.to_path_buf()));
    }
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
