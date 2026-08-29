use super::anchor::{self, ManifestResolver};
use super::lifecycle;
use super::model::{AnchorState, CompactReason, Decision, Decisions, Plan};
use super::rewrite;
use crate::ingest::manifest::{parse_manifest, resolve};
use crate::ingest::model::LegacyEvidence;
use crate::ingest::parse_source;
use crate::kernel::digest::sha256_hex;
use crate::kernel::error::Error;
use jiff::Timestamp;
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

pub(super) struct SourceRecord {
    input: usize,
    line: usize,
    pub(super) id: String,
    pub(super) namespace: String,
    pub(super) type_name: String,
    pub(super) actor: String,
    pub(super) recorded_at: String,
    pub(super) observed_at: String,
    pub(super) valid_at: Option<String>,
    pub(super) payload: Value,
    pub(super) evidence: Vec<LegacyEvidence>,
    pub(super) tags: Vec<String>,
    pub(super) supersedes: Option<String>,
    flattened: Vec<u8>,
}

pub fn build(store_root: &Path, manifest: &Path, at: Timestamp) -> Result<Plan, Error> {
    let manifest_bytes = fs::read(manifest)?;
    let entries = parse_manifest(&manifest_bytes).map_err(compact_error)?;
    let base = manifest.parent().unwrap_or_else(|| Path::new("."));
    let mut sources = Vec::with_capacity(entries.len());
    let mut records = Vec::new();
    let mut ids = HashSet::new();
    let mut resolved_paths = HashSet::new();
    for (input, entry) in entries.iter().enumerate() {
        let source = resolve(base, &entry.path);
        if !source.is_file() {
            return Err(Error::Compact(format!(
                "manifest input is missing: {}",
                entry.path.display()
            )));
        }
        let canonical = fs::canonicalize(&source)?;
        if !resolved_paths.insert(canonical) {
            return Err(Error::Compact(format!(
                "manifest repeats resolved input: {}",
                entry.path.display()
            )));
        }
        let bytes = fs::read(&source)?;
        for parsed in parse_source(&bytes, true).map_err(compact_error)? {
            let mut record = parsed.record;
            let id = record.id.clone();
            if !ids.insert(id.clone()) {
                return Err(Error::Compact(format!("duplicate record id: {id}")));
            }
            let supersedes = record.supersedes.take();
            let flattened =
                serde_json::to_vec(&record).map_err(|error| Error::Compact(error.to_string()))?;
            records.push(SourceRecord {
                input,
                line: parsed.number,
                id,
                namespace: record.namespace,
                type_name: record.type_name,
                actor: record.legacy_actor,
                recorded_at: record.legacy_recorded_at,
                observed_at: record.observed_at,
                valid_at: record.valid_at,
                payload: record.payload,
                evidence: record.evidence,
                tags: record.tags,
                supersedes,
                flattened,
            });
        }
        sources.push((source, bytes));
    }
    lifecycle::validate(store_root, &records)?;
    let superseded = records
        .iter()
        .filter_map(|record| {
            record
                .supersedes
                .as_ref()
                .filter(|id| ids.contains(*id))
                .cloned()
        })
        .collect::<HashSet<_>>();
    let resolvers = entries
        .iter()
        .map(|entry| {
            ManifestResolver::load(
                entry
                    .anchor_resolver
                    .as_ref()
                    .map(|path| resolve(base, path))
                    .as_deref(),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let decisions = decide(&records, &entries, &resolvers, &superseded, at)?;
    let inputs = sources
        .into_iter()
        .enumerate()
        .map(|(index, (source, before))| {
            rewrite::planned_input(index, source, before, &entries[index], &decisions)
        })
        .collect::<Result<Vec<_>, Error>>()?;
    Ok(Plan {
        manifest_sha256: sha256_hex(&manifest_bytes),
        manifest_bytes,
        entries,
        inputs,
    })
}

fn decide(
    records: &[SourceRecord],
    entries: &[crate::ingest::manifest::ManifestEntry],
    resolvers: &[ManifestResolver],
    superseded: &HashSet<String>,
    at: Timestamp,
) -> Result<Decisions, Error> {
    let mut out = Decisions::new();
    for record in records {
        let result = if superseded.contains(&record.id) {
            (true, Some(CompactReason::Superseded))
        } else if let Some(reason) = expiry(record, entries[record.input].expiry.as_ref(), at)? {
            reason
        } else if let Some(anchor) = anchor::from_tags(&record.tags)? {
            match resolvers[record.input].state(&anchor) {
                Some(AnchorState::Dead) => (true, Some(CompactReason::DeadAnchor)),
                Some(AnchorState::Alive) => (false, Some(CompactReason::ActiveAnchor)),
                None => (false, Some(CompactReason::UnknownAnchor)),
            }
        } else if record
            .supersedes
            .as_ref()
            .is_some_and(|target| superseded.contains(target))
        {
            (false, Some(CompactReason::ActiveDescendant))
        } else {
            (false, None)
        };
        out.insert(
            (record.input, record.line),
            Decision {
                id: record.id.clone(),
                remove: result.0,
                reason: result.1,
                replacement: (result.1 == Some(CompactReason::ActiveDescendant))
                    .then(|| record.flattened.clone()),
            },
        );
    }
    Ok(out)
}

fn expiry(
    record: &SourceRecord,
    policy: Option<&crate::ingest::manifest::ExpiryPolicy>,
    at: Timestamp,
) -> Result<Option<(bool, Option<CompactReason>)>, Error> {
    let Some(policy) = policy else {
        return Ok(None);
    };
    let Some(value) = record.payload.pointer(&policy.pointer) else {
        return Ok(None);
    };
    let value = value.as_str().ok_or_else(|| {
        Error::Compact(format!(
            "record {} expiry pointer is not a string",
            record.id
        ))
    })?;
    let expires: Timestamp = value
        .parse()
        .map_err(|_| Error::Compact(format!("record {} expiry is not RFC3339", record.id)))?;
    if expires > at {
        return Ok(None);
    }
    let warning = i64::from(policy.warning_days) * 86_400;
    if expires.as_second().saturating_add(warning) > at.as_second() {
        Ok(Some((false, Some(CompactReason::ExpiryWarningWindow))))
    } else {
        Ok(Some((true, Some(CompactReason::Expired))))
    }
}

fn compact_error(error: Error) -> Error {
    Error::Compact(error.to_string())
}
