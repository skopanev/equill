use crate::kernel::digest::sha256_hex;
use crate::kernel::error::Error;
use crate::record::StoredRecord;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

pub(crate) const SCHEMA: &str = "equill.import-set-receipt.v1";

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ImportSetReceipt {
    pub schema: String,
    pub manifest_sha256: String,
    pub set_sha256: String,
    pub inputs: Vec<InputReceipt>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InputReceipt {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub input_sha256: String,
    pub total: usize,
    pub line_sha256: Vec<String>,
}

pub(crate) fn set_digest(manifest_sha256: &str, inputs: &[InputReceipt]) -> String {
    let material = inputs
        .iter()
        .fold(manifest_sha256.to_owned(), |mut out, input| {
            out.push('\0');
            out.push_str(&input.input_sha256);
            out
        });
    sha256_hex(material.as_bytes())
}

pub(crate) fn persist(store: &Path, receipt: &ImportSetReceipt) -> Result<String, Error> {
    let directory = store.join("receipts/imports");
    fs::create_dir_all(&directory)?;
    let relative = format!("receipts/imports/{}.json", receipt.set_sha256);
    let final_path = store.join(&relative);
    let mut bytes = serde_json::to_vec(receipt)?;
    bytes.push(b'\n');
    if final_path.exists() {
        if fs::read(&final_path)? != bytes {
            return Err(Error::Integrity("import-set receipt hash collision".into()));
        }
        return Ok(relative);
    }
    let temporary = directory.join(format!(
        ".{}.tmp-{}",
        receipt.set_sha256,
        std::process::id()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    fs::rename(temporary, final_path)?;
    Ok(relative)
}

pub(crate) fn verify_receipts(
    store: &Path,
    records: &[StoredRecord],
) -> Result<(usize, usize), Error> {
    let directory = store.join("receipts/imports");
    if !directory.is_dir() {
        return Ok((0, 0));
    }
    let known = records
        .iter()
        .flat_map(|record| &record.evidence)
        .filter(|item| item.kind == "equill.import.line")
        .filter_map(|item| item.sha256.as_deref())
        .collect::<HashSet<_>>();
    let mut receipts = 0;
    let mut inputs = 0;
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let receipt: ImportSetReceipt = serde_json::from_slice(&fs::read(&path)?)?;
        validate(&path, &receipt, &known)?;
        receipts += 1;
        inputs += receipt.inputs.len();
    }
    Ok((receipts, inputs))
}

fn validate(path: &Path, receipt: &ImportSetReceipt, known: &HashSet<&str>) -> Result<(), Error> {
    let expected = set_digest(&receipt.manifest_sha256, &receipt.inputs);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if receipt.schema != SCHEMA || receipt.set_sha256 != expected || stem != expected {
        return Err(Error::Integrity(format!(
            "invalid import-set receipt: {}",
            path.display()
        )));
    }
    for input in &receipt.inputs {
        if input.path.is_empty()
            || input.role.as_ref().is_some_and(|role| !valid_role(role))
            || input.total != input.line_sha256.len()
            || !valid_digest(&input.input_sha256)
        {
            return Err(Error::Integrity(format!(
                "invalid import input receipt: {}",
                input.path
            )));
        }
        if let Some(missing) = input
            .line_sha256
            .iter()
            .find(|digest| !known.contains(digest.as_str()))
        {
            return Err(Error::Integrity(format!(
                "import receipt references missing line sha256 {missing}"
            )));
        }
    }
    Ok(())
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) fn valid_role(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}
