use super::TypeDefinition;
use crate::kernel::digest::sha256_hex;
use crate::kernel::error::Error;
use crate::kernel::governance::RootGuard;
use crate::kernel::lock::StoreLock;
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write as IoWrite;
use std::path::Path;

use super::validation;

#[derive(Debug, Serialize)]
pub struct RegisterReport {
    pub ok: bool,
    pub created: bool,
    pub type_name: String,
    pub sha256: String,
}

pub fn register_file(
    store_root: &Path,
    source: &Path,
    actor: &str,
) -> Result<RegisterReport, Error> {
    let value: serde_json::Value = serde_json::from_slice(&fs::read(source)?)?;
    let definition = if value.get("payload_schema").is_some() {
        serde_json::from_value(value)?
    } else {
        portable_definition(value, actor)?
    };
    register(store_root, definition, actor)
}

fn portable_definition(
    payload_schema: serde_json::Value,
    actor: &str,
) -> Result<TypeDefinition, Error> {
    let type_name = payload_schema
        .pointer("/x-equill-envelope/type")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            Error::InvalidSchema("portable schema requires x-equill-envelope.type".into())
        })?
        .to_owned();
    let uri = payload_schema
        .get("$id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::InvalidSchema("portable schema requires $id".into()))?
        .to_owned();
    Ok(TypeDefinition {
        type_name,
        uri,
        owner: actor.to_owned(),
        payload_schema,
        lifecycle: Default::default(),
    })
}

pub fn register(
    store_root: &Path,
    definition: TypeDefinition,
    actor: &str,
) -> Result<RegisterReport, Error> {
    let (_guard, _config) = RootGuard::acquire(store_root, actor)?;
    register_authorized(store_root, definition)
}

/// The same work for a caller that already holds the governance guard. Taking it
/// twice would deadlock, so an operation inside a governance transaction reaches
/// this entry point instead of the public one.
pub(crate) fn register_authorized(
    store_root: &Path,
    definition: TypeDefinition,
) -> Result<RegisterReport, Error> {
    validate(&definition)?;
    let canonical = serde_json::to_vec(&definition)?;
    let digest = sha256_hex(&canonical);
    let path = store_root
        .join("registry/types")
        .join(format!("{}.json", definition.type_name));
    let _lock = StoreLock::exclusive(store_root)?;

    if path.exists() {
        let current: TypeDefinition = serde_json::from_slice(&fs::read(&path)?)?;
        if current != definition {
            return Err(Error::SchemaConflict(definition.type_name));
        }
        return Ok(report(current.type_name, digest, false));
    }

    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let result = write_definition(&temporary, &definition).and_then(|()| {
        fs::rename(&temporary, &path)?;
        Ok(report(definition.type_name, digest, true))
    });
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn report(type_name: String, sha256: String, created: bool) -> RegisterReport {
    RegisterReport {
        ok: true,
        created,
        type_name,
        sha256,
    }
}

fn write_definition(path: &Path, definition: &TypeDefinition) -> Result<(), Error> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    serde_json::to_writer_pretty(&mut file, definition)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn validate(definition: &TypeDefinition) -> Result<(), Error> {
    validate_type_name(&definition.type_name)?;
    validation::validate(definition)
}

fn validate_type_name(type_name: &str) -> Result<(), Error> {
    validation::validate_type_name(type_name)
}

#[cfg(test)]
mod tests {
    use super::register;
    use crate::command::init;
    use crate::schema::TypeDefinition;
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(1);

    fn store(name: &str) -> PathBuf {
        let suffix = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "equill-schema-registry-{name}-{}-{suffix}",
            std::process::id()
        ));
        init::create(&path, "test-owner", "agent.memory").expect("initialize");
        path
    }

    fn definition(owner: &str) -> TypeDefinition {
        TypeDefinition {
            type_name: "agent.lesson.v1".into(),
            uri: "equill://agent.lesson/v1".into(),
            owner: owner.into(),
            payload_schema: json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": { "rule": { "type": "string" } },
                "required": ["rule"],
                "additionalProperties": false
            }),
            lifecycle: Default::default(),
        }
    }

    #[test]
    fn registration_is_immutable_and_idempotent() {
        let path = store("register");
        let first = register(&path, definition("schema-owner"), "test-owner").expect("register");
        let second = register(&path, definition("schema-owner"), "test-owner").expect("repeat");
        let conflict =
            register(&path, definition("other-owner"), "test-owner").expect_err("conflict");

        assert!(first.created);
        assert!(!second.created);
        assert_eq!(first.sha256, second.sha256);
        assert!(conflict.to_string().contains("registered differently"));
        fs::remove_dir_all(path).expect("remove test store");
    }

    #[test]
    fn rejects_invalid_json_schema() {
        let path = store("invalid");
        let mut item = definition("schema-owner");
        item.payload_schema = json!({ "type": 42 });
        let error = register(&path, item, "test-owner").expect_err("invalid schema");

        assert!(error.to_string().contains("invalid schema"));
        fs::remove_dir_all(path).expect("remove test store");
    }
}
