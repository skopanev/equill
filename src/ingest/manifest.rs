use super::jsonl::import_jsonl_allow_empty;
use super::model::{ImportReport, ImportSetReport};
use super::receipt::{self, ImportSetReceipt, InputReceipt};
use crate::kernel::digest::sha256_hex;
use crate::kernel::error::Error;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManifestEntry {
    pub path: PathBuf,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub expiry: Option<ExpiryPolicy>,
    #[serde(default)]
    pub anchor_resolver: Option<PathBuf>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExpiryPolicy {
    pub pointer: String,
    #[serde(default = "default_warning_days")]
    pub warning_days: u32,
}

pub(crate) struct ResolvedInput {
    pub declared: PathBuf,
    pub role: Option<String>,
    pub source: PathBuf,
}

pub fn import_manifest(
    store: &Path,
    manifest: &Path,
    actor: &str,
) -> Result<ImportSetReport, Error> {
    let manifest_bytes = fs::read(manifest)?;
    let entries = parse_manifest(&manifest_bytes)?;
    let base = manifest.parent().unwrap_or_else(|| Path::new("."));
    let resolved = entries
        .into_iter()
        .map(|entry| ResolvedInput {
            declared: entry.path.clone(),
            role: entry.role,
            source: resolve(base, &entry.path),
        })
        .collect();
    import_resolved(store, &manifest_bytes, resolved, actor)
}

pub(crate) fn import_resolved(
    store: &Path,
    manifest_bytes: &[u8],
    entries: Vec<ResolvedInput>,
    actor: &str,
) -> Result<ImportSetReport, Error> {
    let manifest_sha256 = sha256_hex(manifest_bytes);
    let mut reports = Vec::with_capacity(entries.len());
    let mut receipts = Vec::with_capacity(entries.len());
    for entry in entries {
        let report = import_jsonl_allow_empty(store, &entry.source, actor)?;
        receipts.push(input_receipt(
            &entry.declared,
            entry.role.as_deref(),
            &report,
        )?);
        reports.push(report);
    }
    let set_sha256 = receipt::set_digest(&manifest_sha256, &receipts);
    let receipt = ImportSetReceipt {
        schema: receipt::SCHEMA.into(),
        manifest_sha256: manifest_sha256.clone(),
        set_sha256: set_sha256.clone(),
        inputs: receipts,
    };
    let relative = receipt::persist(store, &receipt)?;
    Ok(report(manifest_sha256, set_sha256, relative, &reports))
}

pub(crate) fn parse_manifest(bytes: &[u8]) -> Result<Vec<ManifestEntry>, Error> {
    let text = std::str::from_utf8(bytes)
        .map_err(|error| Error::Import(format!("import manifest is not UTF-8: {error}")))?;
    let mut entries = Vec::new();
    let mut paths = std::collections::HashSet::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let entry: ManifestEntry = serde_json::from_str(line)
            .map_err(|error| Error::Import(format!("manifest line {}: {error}", index + 1)))?;
        if entry.path.as_os_str().is_empty() {
            return Err(Error::Import(format!(
                "manifest line {} has empty path",
                index + 1
            )));
        }
        if entry
            .role
            .as_ref()
            .is_some_and(|role| !receipt::valid_role(role))
        {
            return Err(Error::Import(format!(
                "manifest line {} has invalid role",
                index + 1
            )));
        }
        if entry
            .expiry
            .as_ref()
            .is_some_and(|policy| !policy.pointer.starts_with('/') || policy.pointer.len() > 500)
        {
            return Err(Error::Import(format!(
                "manifest line {} has invalid expiry pointer",
                index + 1
            )));
        }
        if !paths.insert(entry.path.clone()) {
            return Err(Error::Import(format!(
                "manifest line {} repeats a path",
                index + 1
            )));
        }
        entries.push(entry);
    }
    if entries.is_empty() {
        return Err(Error::Import("import manifest contains no paths".into()));
    }
    Ok(entries)
}

pub(crate) fn resolve(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

const fn default_warning_days() -> u32 {
    30
}

fn input_receipt(
    path: &Path,
    role: Option<&str>,
    report: &ImportReport,
) -> Result<InputReceipt, Error> {
    let path = path
        .to_str()
        .ok_or_else(|| Error::Import("manifest path is not UTF-8".into()))?;
    Ok(InputReceipt {
        path: path.into(),
        role: role.map(str::to_owned),
        input_sha256: report.input_sha256.clone(),
        total: report.total,
        line_sha256: report
            .records
            .iter()
            .map(|row| row.line_sha256.clone())
            .collect(),
    })
}

fn report(
    manifest_sha256: String,
    set_sha256: String,
    receipt: String,
    reports: &[ImportReport],
) -> ImportSetReport {
    ImportSetReport {
        ok: true,
        manifest_sha256,
        set_sha256,
        inputs: reports.len(),
        total: reports.iter().map(|item| item.total).sum(),
        imported: reports.iter().map(|item| item.imported).sum(),
        skipped: reports.iter().map(|item| item.skipped).sum(),
        receipt,
    }
}
