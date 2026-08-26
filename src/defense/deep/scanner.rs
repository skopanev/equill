use super::model::DeepFinding;
use crate::defense::provider::secrets_scanner;
use crate::kernel::digest::sha256_hex;
use crate::kernel::error::Error;
use crate::kernel::lock::StoreLock;
use crate::record::StoredRecord;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

#[derive(Serialize)]
struct ScanBody<'a> {
    payload: &'a serde_json::Value,
    evidence: &'a [crate::record::EvidenceRef],
    tags: &'a [String],
}

pub fn audit(store_root: &Path) -> Result<super::DeepReport, Error> {
    let _lock = StoreLock::exclusive(store_root)?;
    let custom_rules = crate::defense::policy::custom_rules(store_root)?;
    let mut ledgers = ledgers(store_root)?;
    ledgers.sort();
    let mut digest = Sha256::new();
    digest.update(b"equill-deep-defense-v1\0");
    digest.update(custom_rules.as_deref().unwrap_or("").as_bytes());
    let mut findings = Vec::new();
    let mut records = 0;
    for path in ledgers {
        scan_ledger(
            store_root,
            &path,
            custom_rules.as_deref(),
            &mut digest,
            &mut records,
            &mut findings,
        )?;
    }
    findings.sort_by(|left, right| {
        (
            left.record_id,
            &left.rule,
            left.content_line,
            left.content_column,
        )
            .cmp(&(
                right.record_id,
                &right.rule,
                right.content_line,
                right.content_column,
            ))
    });
    findings.dedup_by(|left, right| {
        left.record_id == right.record_id
            && left.rule == right.rule
            && left.content_line == right.content_line
            && left.content_column == right.content_column
    });
    let corpus_sha256 = hex(&digest.finalize());
    let scan_id = sha256_hex(format!("secrets-scanner-0.2.3\0{corpus_sha256}").as_bytes());
    super::receipt::persist(store_root, &scan_id, &corpus_sha256, records, &findings)
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

fn ledgers(store_root: &Path) -> Result<Vec<PathBuf>, Error> {
    let root = store_root.join("records");
    let mut paths = Vec::new();
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.extension().is_some_and(|value| value == "jsonl") {
            paths.push(path);
        }
    }
    Ok(paths)
}

fn scan_ledger(
    store_root: &Path,
    path: &Path,
    custom_rules: Option<&str>,
    digest: &mut Sha256,
    records: &mut usize,
    findings: &mut Vec<DeepFinding>,
) -> Result<(), Error> {
    let ledger = path
        .strip_prefix(store_root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned();
    digest.update(ledger.as_bytes());
    for (index, line) in BufReader::new(File::open(path)?).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        digest.update(line.as_bytes());
        let record: StoredRecord = serde_json::from_str(&line)
            .map_err(|error| Error::Integrity(format!("{ledger}:{}: {error}", index + 1)))?;
        let body = serde_json::to_string(&ScanBody {
            payload: &record.payload,
            evidence: &record.evidence,
            tags: &record.tags,
        })?;
        append_findings(&record, &ledger, index + 1, &body, custom_rules, findings)?;
        *records += 1;
    }
    Ok(())
}

fn append_findings(
    record: &StoredRecord,
    ledger: &str,
    ledger_line: usize,
    body: &str,
    custom_rules: Option<&str>,
    output: &mut Vec<DeepFinding>,
) -> Result<(), Error> {
    let mut matches = secrets_scanner::scan_deep(body)?.matches;
    if let Some(rules) = custom_rules {
        matches.extend(secrets_scanner::scan_custom(rules, body)?.matches);
    }
    for item in matches {
        output.push(DeepFinding {
            record_id: record.id,
            ledger: ledger.to_owned(),
            ledger_line,
            rule: item.rule,
            content_line: item.line,
            content_column: item.column,
        });
    }
    Ok(())
}
