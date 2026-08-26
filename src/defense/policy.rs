use super::model::{DefenseMode, DefensePolicy};
use crate::kernel::error::Error;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

const POLICY_PATH: &str = "registry/defense/policy.json";
const RULES_PATH: &str = "registry/defense/rules.toml";

pub fn initialize(store_root: &Path) -> Result<(), Error> {
    let policy = DefensePolicy {
        mode: DefenseMode::Block,
    };
    let path = store_root.join(POLICY_PATH);
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    serde_json::to_writer_pretty(&mut file, &policy)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    let mut rules = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(store_root.join(RULES_PATH))?;
    rules.write_all(b"title = \"Equill store rules\"\n")?;
    rules.sync_all()?;
    Ok(())
}

pub fn load(store_root: &Path) -> Result<DefensePolicy, Error> {
    let path = store_root.join(POLICY_PATH);
    let policy: DefensePolicy =
        serde_json::from_slice(&fs::read(&path).map_err(|error| {
            Error::Integrity(format!("cannot read {}: {error}", path.display()))
        })?)?;
    Ok(policy)
}

pub fn custom_rules(store_root: &Path) -> Result<Option<String>, Error> {
    let path = store_root.join(RULES_PATH);
    let contents = fs::read_to_string(&path)
        .map_err(|error| Error::Integrity(format!("cannot read {}: {error}", path.display())))?;
    Ok(contents.contains("[[rules]]").then_some(contents))
}
