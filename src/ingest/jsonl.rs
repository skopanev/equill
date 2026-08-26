use super::model::{ImportItem, ImportReport, ImportStatus, LegacyEvidence, LegacyRecord};
use crate::kernel::digest::sha256_hex;
use crate::kernel::error::Error;
use crate::record::{self, EvidenceRef, RecordDraft};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use uuid::Uuid;

const IMPORT_KIND: &str = "equill.import.line";
const LEGACY_ID_KIND: &str = "legacy.record-id";

pub fn import_jsonl(store: &Path, input: &Path, actor: &str) -> Result<ImportReport, Error> {
    let bytes = fs::read(input)?;
    let input_sha256 = sha256_hex(&bytes);
    let contents = std::str::from_utf8(&bytes)
        .map_err(|error| Error::Import(format!("input is not UTF-8: {error}")))?;
    let lines = parse_lines(contents)?;
    let mut known = known_imports(store)?;
    let mut records = Vec::with_capacity(lines.len());
    let mut imported = 0;
    let mut skipped = 0;

    for (line, source, digest) in lines {
        if let Some(record_id) = known.by_digest.get(&digest).copied() {
            known.by_legacy.insert(source.id.clone(), record_id);
            records.push(item(line, source.id, record_id, ImportStatus::Skipped));
            skipped += 1;
            continue;
        }
        if known.by_legacy.contains_key(&source.id) {
            return Err(Error::Import(format!(
                "line {line}: legacy id {} was already imported with different content",
                source.id
            )));
        }
        let legacy_id = source.id.clone();
        let draft = draft(source, &digest, &known.by_legacy)
            .map_err(|error| Error::Import(format!("line {line}: {error}")))?;
        let report = record::append(store, draft, actor)
            .map_err(|error| Error::Import(format!("line {line}: {error}")))?;
        known.by_digest.insert(digest, report.id);
        known.by_legacy.insert(legacy_id.clone(), report.id);
        records.push(item(line, legacy_id, report.id, ImportStatus::Imported));
        imported += 1;
    }
    Ok(ImportReport {
        ok: true,
        input_sha256,
        total: records.len(),
        imported,
        skipped,
        records,
    })
}

fn parse_lines(contents: &str) -> Result<Vec<(usize, LegacyRecord, String)>, Error> {
    let mut records = Vec::new();
    for (index, line) in contents.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let source: LegacyRecord = serde_json::from_str(line)
            .map_err(|error| Error::Import(format!("line {}: {error}", index + 1)))?;
        records.push((index + 1, source, sha256_hex(line.as_bytes())));
    }
    if records.is_empty() {
        return Err(Error::Import("input contains no records".into()));
    }
    Ok(records)
}

struct KnownImports {
    by_digest: HashMap<String, Uuid>,
    by_legacy: HashMap<String, Uuid>,
}

fn known_imports(store: &Path) -> Result<KnownImports, Error> {
    let mut known = KnownImports {
        by_digest: HashMap::new(),
        by_legacy: HashMap::new(),
    };
    for record in record::read_all(store)? {
        for evidence in record.evidence {
            if evidence.kind == IMPORT_KIND {
                if let Some(digest) = evidence.sha256 {
                    known.by_digest.insert(digest, record.id);
                }
            } else if evidence.kind == LEGACY_ID_KIND {
                known.by_legacy.insert(evidence.reference, record.id);
            }
        }
    }
    Ok(known)
}

fn draft(
    source: LegacyRecord,
    digest: &str,
    legacy_ids: &HashMap<String, Uuid>,
) -> Result<RecordDraft, Error> {
    let mut evidence = source
        .evidence
        .into_iter()
        .map(|item| match item {
            LegacyEvidence::Text(reference) => EvidenceRef {
                kind: "legacy.evidence".into(),
                reference,
                sha256: None,
            },
            LegacyEvidence::Typed(reference) => reference,
        })
        .collect::<Vec<_>>();
    evidence.extend([
        EvidenceRef {
            kind: IMPORT_KIND.into(),
            reference: "legacy-jsonl".into(),
            sha256: Some(digest.into()),
        },
        EvidenceRef {
            kind: LEGACY_ID_KIND.into(),
            reference: source.id,
            sha256: None,
        },
        EvidenceRef {
            kind: "legacy.recorded-at".into(),
            reference: source.legacy_recorded_at,
            sha256: None,
        },
        EvidenceRef {
            kind: "legacy.actor".into(),
            reference: source.legacy_actor,
            sha256: None,
        },
    ]);
    let supersedes = source
        .supersedes
        .map(|id| resolve_supersedes(&id, legacy_ids))
        .transpose()?;
    Ok(RecordDraft {
        namespace: source.namespace,
        type_name: source.type_name,
        observed_at: source.observed_at,
        valid_at: source.valid_at,
        payload: source.payload,
        evidence,
        tags: source.tags,
        supersedes,
    })
}

fn resolve_supersedes(value: &str, legacy_ids: &HashMap<String, Uuid>) -> Result<Uuid, Error> {
    Uuid::parse_str(value)
        .ok()
        .or_else(|| legacy_ids.get(value).copied())
        .ok_or_else(|| Error::Import(format!("supersedes target is unknown: {value}")))
}

fn item(line: usize, legacy_id: String, record_id: Uuid, status: ImportStatus) -> ImportItem {
    ImportItem {
        line,
        legacy_id,
        record_id,
        status,
    }
}
