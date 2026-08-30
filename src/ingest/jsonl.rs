use super::model::{ImportItem, ImportReport, ImportStatus, LegacyEvidence, LegacyRecord};
use crate::kernel::digest::sha256_hex;
use crate::kernel::error::Error;
use crate::record::{self, EvidenceRef, RecordDraft};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use uuid::Uuid;

const IMPORT_KIND: &str = "equill.import.line";
const LEGACY_ID_KIND: &str = "legacy.record-id";

pub(crate) struct ParsedLine {
    pub number: usize,
    pub record: LegacyRecord,
    pub digest: String,
}

pub fn import_jsonl(store: &Path, input: &Path, actor: &str) -> Result<ImportReport, Error> {
    import_jsonl_inner(store, input, actor, false)
}

pub(crate) fn import_jsonl_allow_empty(
    store: &Path,
    input: &Path,
    actor: &str,
) -> Result<ImportReport, Error> {
    import_jsonl_inner(store, input, actor, true)
}

fn import_jsonl_inner(
    store: &Path,
    input: &Path,
    actor: &str,
    allow_empty: bool,
) -> Result<ImportReport, Error> {
    let bytes = fs::read(input)?;
    let input_sha256 = sha256_hex(&bytes);
    let lines = parse_source(&bytes, allow_empty)?;
    let mut known = known_imports(store)?;
    let mut records = Vec::with_capacity(lines.len());
    let mut imported = 0;
    let mut skipped = 0;

    for parsed in lines {
        let ParsedLine {
            number: line,
            record: source,
            digest,
        } = parsed;
        if let Some(record_id) = known.by_digest.get(&digest).copied() {
            known.by_legacy.insert(source.id.clone(), record_id);
            records.push(item(
                line,
                digest,
                source.id,
                record_id,
                ImportStatus::Skipped,
            ));
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
        let draft = draft(source, &digest, &known.by_legacy, &known.ids)
            .map_err(|error| Error::Import(format!("line {line}: {error}")))?;
        let report = record::append_only(store, draft, actor)
            .map_err(|error| Error::Import(format!("line {line}: {error}")))?;
        known.by_digest.insert(digest.clone(), report.id);
        known.by_legacy.insert(legacy_id.clone(), report.id);
        known.ids.insert(report.id);
        records.push(item(
            line,
            digest,
            legacy_id,
            report.id,
            ImportStatus::Imported,
        ));
        imported += 1;
    }
    // One catch-up for the whole set: forty records should cost one model load,
    // not forty. A partial import still publishes what actually committed.
    if imported > 0 {
        crate::vector::after_commit(store, actor);
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

pub(crate) fn parse_source(bytes: &[u8], allow_empty: bool) -> Result<Vec<ParsedLine>, Error> {
    let contents = std::str::from_utf8(bytes)
        .map_err(|error| Error::Import(format!("input is not UTF-8: {error}")))?;
    let mut records = Vec::new();
    for (index, line) in contents.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let source: LegacyRecord = serde_json::from_str(line)
            .map_err(|error| Error::Import(format!("line {}: {error}", index + 1)))?;
        records.push(ParsedLine {
            number: index + 1,
            record: source,
            digest: sha256_hex(line.as_bytes()),
        });
    }
    if records.is_empty() && !allow_empty {
        return Err(Error::Import("input contains no records".into()));
    }
    Ok(records)
}

struct KnownImports {
    by_digest: HashMap<String, Uuid>,
    by_legacy: HashMap<String, Uuid>,
    /// Every record id already in the store. A `supersedes` written as a raw
    /// uuid is only honoured when it names one of these.
    ids: HashSet<Uuid>,
}

fn known_imports(store: &Path) -> Result<KnownImports, Error> {
    let mut known = KnownImports {
        by_digest: HashMap::new(),
        by_legacy: HashMap::new(),
        ids: HashSet::new(),
    };
    for record in record::read_all(store)? {
        known.ids.insert(record.id);
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
    known_ids: &HashSet<Uuid>,
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
        .map(|id| resolve_supersedes(&id, legacy_ids, known_ids))
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

/// The legacy id map wins: it is this run's own source-id to record-id mapping.
/// A raw uuid is accepted only when a record with that id actually exists —
/// otherwise re-importing an exported ledger would store dangling pointers and
/// silently drop every supersession.
fn resolve_supersedes(
    value: &str,
    legacy_ids: &HashMap<String, Uuid>,
    known_ids: &HashSet<Uuid>,
) -> Result<Uuid, Error> {
    legacy_ids
        .get(value)
        .copied()
        .or_else(|| {
            Uuid::parse_str(value)
                .ok()
                .filter(|id| known_ids.contains(id))
        })
        .ok_or_else(|| Error::Import(format!("supersedes target is unknown: {value}")))
}

fn item(
    line: usize,
    line_sha256: String,
    legacy_id: String,
    record_id: Uuid,
    status: ImportStatus,
) -> ImportItem {
    ImportItem {
        line,
        line_sha256,
        legacy_id,
        record_id,
        status,
    }
}
