use crate::defense;
use crate::kernel::error::Error;
use crate::kernel::identity;
use serde_json::{Value, json};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const DIRECTORIES: &[&str] = &[
    "locks",
    "records",
    "objects",
    "receipts",
    "projections",
    "registry/types",
    "registry/selectors",
    "registry/profiles",
    "registry/gates",
    "registry/defense",
];

pub fn create(store: &Path, owner: &str, namespace: &str) -> Result<Value, Error> {
    let owner = owner.trim();
    let namespace = namespace.trim();
    if !identity::valid(owner) {
        return Err(Error::InvalidOwner);
    }
    if !valid_namespace(namespace) {
        return Err(Error::InvalidNamespace);
    }

    if store.join("store.json").exists() {
        return existing_store(store, owner, namespace);
    }
    if store.exists() {
        return Err(Error::StoreExists(store.to_path_buf()));
    }

    let staging = staging_path(store)?;
    fs::create_dir(&staging)?;
    let result = initialize_staging(&staging, owner, namespace).and_then(|report| {
        fs::rename(&staging, store)?;
        Ok(report)
    });
    if result.is_err() && staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

fn valid_namespace(namespace: &str) -> bool {
    !namespace.is_empty()
        && namespace.split('.').all(|part| {
            !part.is_empty()
                && part.chars().all(|character| {
                    character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
                })
        })
}

fn existing_store(store: &Path, owner: &str, namespace: &str) -> Result<Value, Error> {
    let metadata: Value = serde_json::from_slice(&fs::read(store.join("store.json"))?)?;
    if metadata["root_owner"] != owner || metadata["namespaces"][0] != namespace {
        return Err(Error::StoreMismatch);
    }
    Ok(json!({ "ok": true, "created": false }))
}

fn staging_path(store: &Path) -> Result<PathBuf, Error> {
    let parent = store
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let name = store
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("store");
    let staging = parent.join(format!(".{name}.init-{}", std::process::id()));
    if staging.exists() {
        return Err(Error::StoreExists(staging));
    }
    Ok(staging)
}

fn initialize_staging(staging: &Path, owner: &str, namespace: &str) -> Result<Value, Error> {
    for directory in DIRECTORIES {
        fs::create_dir_all(staging.join(directory))?;
    }
    defense::initialize(staging)?;
    let created_at_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let metadata = json!({
        "format_version": 1,
        "root_owner": owner,
        "namespaces": [namespace],
        "created_at_unix_ms": created_at_unix_ms,
    });
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(staging.join("store.json"))?;
    serde_json::to_writer_pretty(&mut file, &metadata)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(json!({ "ok": true, "created": true }))
}

#[cfg(test)]
mod tests {
    use super::create;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(1);

    fn target(name: &str) -> PathBuf {
        let suffix = NEXT.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("equill-{name}-{}-{suffix}", std::process::id()))
    }

    #[test]
    fn initializes_store_and_is_idempotent() {
        let path = target("init");
        let first = create(&path, "test-owner", "agent.memory").expect("initialize");
        let second = create(&path, "test-owner", "agent.memory").expect("repeat");

        assert_eq!(first["created"], true);
        assert_eq!(second["created"], false);
        assert!(path.join("records").is_dir());
        assert!(path.join("registry/types").is_dir());
        fs::remove_dir_all(path).expect("remove test store");
    }

    #[test]
    fn rejects_identity_change() {
        let path = target("identity");
        create(&path, "first-owner", "agent.memory").expect("initialize");
        let error = create(&path, "second-owner", "agent.memory").expect_err("reject owner change");

        assert!(error.to_string().contains("different ownership"));
        fs::remove_dir_all(path).expect("remove test store");
    }
}
