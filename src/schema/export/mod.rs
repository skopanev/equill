//! Portable definitions only: no records, client policy, or source-store snapshot.
mod lineage;
mod publish;
#[cfg(test)]
mod tests;

use super::TypeDefinition;
use crate::kernel::{digest::sha256_hex, error::Error};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct ExportReport {
    pub ok: bool,
    pub exported: usize,
    pub current: usize,
    pub legacy: usize,
    pub manifest: &'static str,
}

#[derive(Serialize)]
struct Manifest {
    schema: &'static str,
    entries: Vec<Entry>,
}

#[derive(Serialize)]
struct Entry {
    #[serde(rename = "type")]
    type_name: String,
    filename: String,
    sha256: String,
    status: &'static str,
}

/// Export a validated registry snapshot without changing the source store.
pub fn export(store: &Path, output: &Path, all_registered: bool) -> Result<ExportReport, Error> {
    let definitions = load(store)?;
    let current = lineage::current(&definitions)?;
    let mut files = Vec::new();
    let mut entries = Vec::new();
    for (name, definition) in definitions {
        let effective = current.contains(&name);
        if !all_registered && !effective {
            continue;
        }
        let filename = format!("{name}.json");
        let bytes = canonical(&definition)?;
        entries.push(Entry {
            type_name: name,
            filename: filename.clone(),
            sha256: sha256_hex(&bytes),
            status: if effective { "current" } else { "legacy" },
        });
        files.push((filename, bytes));
    }
    let exported = entries.len();
    files.push((
        "manifest.json".into(),
        canonical(&Manifest {
            schema: "equill.schema-export.v1",
            entries,
        })?,
    ));
    publish::bundle(store, output, &files)?;
    Ok(ExportReport {
        ok: true,
        exported,
        current: current.len(),
        legacy: exported - current.len(),
        manifest: "manifest.json",
    })
}

fn load(store: &Path) -> Result<BTreeMap<String, TypeDefinition>, Error> {
    let directory = store.join("registry/types");
    let entries = fs::read_dir(directory).map_err(|_| failure("registry is unreadable"))?;
    let mut definitions = BTreeMap::new();
    for entry in entries {
        let entry = entry.map_err(|_| failure("registry entry is unreadable"))?;
        if !entry
            .file_type()
            .map_err(|_| failure("registry entry is unreadable"))?
            .is_file()
        {
            return Err(failure("registry entry is not a regular definition"));
        }
        let bytes = fs::read(entry.path()).map_err(|_| failure("definition is unreadable"))?;
        let definition: TypeDefinition =
            serde_json::from_slice(&bytes).map_err(|_| failure("definition is malformed"))?;
        super::validation::validate(&definition).map_err(|_| failure("definition is invalid"))?;
        if entry.file_name() != format!("{}.json", definition.type_name).as_str() {
            return Err(failure("definition filename does not match its type"));
        }
        if definitions
            .insert(definition.type_name.clone(), definition)
            .is_some()
        {
            return Err(failure("duplicate registered type"));
        }
    }
    Ok(definitions)
}

fn canonical(value: &impl Serialize) -> Result<Vec<u8>, Error> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| failure("serialization failed"))?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn failure(message: &str) -> Error {
    Error::InvalidSchema(format!("schema export: {message}"))
}
