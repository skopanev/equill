//! Selectors that ask the index for a type the index was told not to hold.
//!
//! `embed_types` narrows what is embedded. A selector with a vector strategy
//! over a type outside that list is not an error anywhere — it simply returns
//! nothing, which reads to whoever wrote the profile as "no relevant memory"
//! rather than "never indexed". That is the failure this exists to make
//! visible: a wrong answer nobody has any reason to doubt.
use super::model::vector_error;
use crate::context::Strategy;
use crate::kernel::digest::sha256_hex;
use crate::kernel::error::Error;
use crate::record::{StoredRecord, withdrawn};
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use uuid::Uuid;

/// Which types this store embeds, read without verifying model artifacts.
///
/// What a store *would* embed is a property of its registry, not of whether
/// its model files are present and hash correctly. A status report has to
/// answer while the model is missing, and the corpus is counted on paths that
/// never load a model at all — so this reads the one field and nothing else.
pub(crate) fn embed_types(store: &Path) -> Result<Vec<String>, Error> {
    let path = store.join(super::config::CONFIG);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    #[derive(Deserialize)]
    struct Filter {
        #[serde(default)]
        embed_types: Vec<String>,
    }
    let filter: Filter = serde_json::from_slice(&fs::read(path)?)?;
    Ok(filter.embed_types)
}

/// A type name has to be one, whatever the registry happens to hold today.
pub(crate) fn validate_names(names: &[String]) -> Result<(), Error> {
    if names
        .iter()
        .any(|name| name.trim().is_empty() || name.chars().any(char::is_control))
    {
        return Err(super::model::vector_error(
            "embed_types entries must be non-empty type names",
        ));
    }
    Ok(())
}

/// Every registered selector, whether or not a profile names it. A selector
/// nobody uses yet is exactly the one a coverage check has to see.
fn selector_ids(store_root: &Path) -> Result<Vec<String>, Error> {
    let directory = store_root.join("registry/selectors");
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    let mut ids = std::fs::read_dir(directory)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|value| value == "json"))
        .filter_map(|path| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_owned)
        })
        .collect::<Vec<_>>();
    ids.sort();
    Ok(ids)
}

#[derive(Debug, Serialize)]
pub struct Uncovered {
    pub selector: String,
    #[serde(rename = "type")]
    pub type_name: String,
}

/// Every registered selector that would search vectors for a type this store
/// does not embed. Empty when no filter is configured, because then there is
/// no type the index was told to leave out.
pub fn uncovered(store_root: &Path) -> Result<Vec<Uncovered>, Error> {
    let embed_types = embed_types(store_root)?;
    if embed_types.is_empty() {
        return Ok(Vec::new());
    }
    let mut found = Vec::new();
    for id in selector_ids(store_root)? {
        // A selector that will not load is a fault the health check already
        // reports on its own terms; it is not evidence about coverage.
        let Ok((selector, _)) = crate::context::load_selector(store_root, &id) else {
            continue;
        };
        let searches_vectors = selector.strategies.contains(&Strategy::Hybrid);
        if searches_vectors && !embed_types.contains(&selector.type_name) {
            found.push(Uncovered {
                selector: selector.id,
                type_name: selector.type_name,
            });
        }
    }
    Ok(found)
}

/// Records the engine writes about itself. Their payload is digests, so there
/// is nothing to embed and nothing a semantic query could usefully match. They
/// are excluded from the corpus rather than indexed, which also keeps a
/// governance change from leaving every store lagging until someone runs a sync.
fn embeddable(record: &StoredRecord) -> bool {
    record.type_name != crate::governance::AUDIT_TYPE
        && record.type_name != crate::governance::AUDIT_TYPE_V2
}

pub(crate) struct CorpusSnapshot {
    pub(crate) records: Vec<(StoredRecord, String)>,
    pub(crate) digest: String,
    /// What the index must not keep: replaced, withdrawn, and live records of
    /// a type this store no longer embeds. Narrowing `embed_types` without this
    /// leaves their vectors answering searches nothing in the ledger accounts
    /// for.
    pub(crate) history: Vec<Uuid>,
    /// Live records the filter left out, so an operator can see it doing
    /// something rather than infer it from a number that got smaller.
    pub(crate) skipped_by_type: usize,
}

pub(crate) fn corpus(store_root: &Path) -> Result<(Vec<(StoredRecord, String)>, String), Error> {
    let snapshot = corpus_snapshot(store_root)?;
    Ok((snapshot.records, snapshot.digest))
}

/// The corpus every path agrees on.
///
/// The ledger is the truth being indexed, so the digest covers exactly what a
/// canonical read returns: every record hash in record-id order.
///
/// Rebuild, the incremental sync and the status report all read it here, so the
/// filter is honoured by all three by construction rather than by three callers
/// remembering to. The digest covers the filtered corpus, which is what leaves
/// the index behind when `embed_types` narrows.
pub(crate) fn corpus_snapshot(store_root: &Path) -> Result<CorpusSnapshot, Error> {
    let embed_types = embed_types(store_root)?;
    let all = crate::record::read_all(store_root)?;
    let replaced = all
        .iter()
        .filter_map(|record| record.supersedes)
        .collect::<HashSet<_>>();
    let mut history = all
        .iter()
        .filter(|record| replaced.contains(&record.id) || withdrawn(record))
        .map(|record| record.id)
        .collect::<HashSet<_>>();
    let live = all
        .into_iter()
        .filter(embeddable)
        .filter(|record| !history.contains(&record.id))
        .collect::<Vec<_>>();
    let mut skipped_by_type = 0;
    let mut validated = Vec::with_capacity(live.len());
    for record in live {
        if wanted(&embed_types, &record.type_name) {
            validated.push(record);
        } else {
            skipped_by_type += 1;
            history.insert(record.id);
        }
    }
    let mut digests = std::collections::HashMap::new();
    for entry in fs::read_dir(store_root.join("records"))? {
        let path = entry?.path();
        for line in fs::read_to_string(&path)?.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let record: StoredRecord = serde_json::from_str(line)?;
            digests.insert(record.id, sha256_hex(line.as_bytes()));
        }
    }
    let mut records = validated
        .into_iter()
        .map(|record| {
            let digest = digests
                .get(&record.id)
                .cloned()
                .ok_or_else(|| vector_error("record hash is missing from the ledger"))?;
            Ok((record, digest))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    records.sort_by_key(|(record, _)| record.id);
    let mut accumulator = String::new();
    for (_, digest) in &records {
        accumulator.push_str(digest);
    }
    Ok(CorpusSnapshot {
        records,
        digest: sha256_hex(accumulator.as_bytes()),
        history: history.into_iter().collect(),
        skipped_by_type,
    })
}

/// An empty list is not a filter that matches nothing — it is the absence of a
/// filter, and the behaviour of every store written before the field existed.
fn wanted(embed_types: &[String], type_name: &str) -> bool {
    embed_types.is_empty() || embed_types.iter().any(|name| name == type_name)
}
