use super::model::{DefenseMode, DefensePolicy, LiteralPattern};
use crate::kernel::error::Error;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

const POLICY_PATH: &str = "registry/defense/policy.json";

pub fn initialize(store_root: &Path) -> Result<(), Error> {
    let policy = DefensePolicy {
        mode: DefenseMode::Block,
        sensitive_keys: [
            "password",
            "passwd",
            "secret",
            "token",
            "access_token",
            "api_key",
            "apikey",
            "private_key",
        ]
        .map(str::to_owned)
        .into(),
        literal_patterns: [
            ("private-key-pem", "-----BEGIN PRIVATE KEY-----"),
            ("rsa-private-key-pem", "-----BEGIN RSA PRIVATE KEY-----"),
            ("openssh-private-key", "-----BEGIN OPENSSH PRIVATE KEY-----"),
            ("url-token", "token="),
            ("url-api-key", "api_key="),
            ("url-password", "password="),
        ]
        .map(|(id, literal)| LiteralPattern {
            id: id.to_owned(),
            literal: literal.to_owned(),
        })
        .into(),
    };
    let path = store_root.join(POLICY_PATH);
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    serde_json::to_writer_pretty(&mut file, &policy)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

pub fn load(store_root: &Path) -> Result<DefensePolicy, Error> {
    let path = store_root.join(POLICY_PATH);
    let policy: DefensePolicy = serde_json::from_slice(&fs::read(&path).map_err(|error| {
        Error::Integrity(format!("cannot read {}: {error}", path.display()))
    })?)?;
    validate(&policy)?;
    Ok(policy)
}

fn validate(policy: &DefensePolicy) -> Result<(), Error> {
    if policy.sensitive_keys.iter().any(|key| key.trim().is_empty()) {
        return Err(Error::Integrity(
            "memory-defense sensitive keys cannot be empty".into(),
        ));
    }
    if policy.literal_patterns.iter().any(|pattern| {
        pattern.id.trim().is_empty()
            || pattern.literal.len() < 4
            || pattern.id.chars().any(char::is_control)
    }) {
        return Err(Error::Integrity(
            "memory-defense patterns require an id and a literal of at least 4 bytes".into(),
        ));
    }
    Ok(())
}
