use super::StoredRecord;
use crate::kernel::error::Error;
use serde::Serialize;
use serde_json::Value;
use std::path::Path;

/// A near-duplicate the store already holds. This is a warning attached to a
/// successful write, never a refusal: the whole point of the ledger is to stop
/// people rediscovering what is written down, and the author is the one who can
/// tell a duplicate from a deliberate restatement.
#[derive(Debug, Serialize)]
pub struct SimilarRecord {
    pub id: uuid::Uuid,
    pub overlap: u8,
}

/// Compares against records of the same type and namespace only. Similarity is
/// word overlap over the payload's own text — crude on purpose: a cheap check
/// that runs on every write is worth more than an accurate one nobody enables.
pub fn find(store_root: &Path, candidate: &StoredRecord) -> Result<Vec<SimilarRecord>, Error> {
    const THRESHOLD: u8 = 70;
    let candidate_words = words(&candidate.payload);
    if candidate_words.is_empty() {
        return Ok(Vec::new());
    }
    let mut similar = crate::record::read_all(store_root)?
        .into_iter()
        .filter(|record| record.id != candidate.id)
        .filter(|record| {
            record.namespace == candidate.namespace && record.type_name == candidate.type_name
        })
        .filter_map(|record| {
            let overlap = overlap(&candidate_words, &words(&record.payload));
            (overlap >= THRESHOLD).then_some(SimilarRecord {
                id: record.id,
                overlap,
            })
        })
        .collect::<Vec<_>>();
    similar.sort_by_key(|item| std::cmp::Reverse(item.overlap));
    similar.truncate(3);
    Ok(similar)
}

fn overlap(left: &[String], right: &[String]) -> u8 {
    if left.is_empty() || right.is_empty() {
        return 0;
    }
    let shared = left.iter().filter(|word| right.contains(word)).count();
    let total = left.len().max(right.len());
    ((shared * 100) / total) as u8
}

fn words(payload: &Value) -> Vec<String> {
    let mut words = Vec::new();
    collect(payload, &mut words);
    words.sort();
    words.dedup();
    words
}

fn collect(value: &Value, words: &mut Vec<String>) {
    match value {
        Value::String(text) => words.extend(
            text.split_whitespace()
                .map(|word| {
                    word.trim_matches(|c: char| !c.is_alphanumeric())
                        .to_lowercase()
                })
                .filter(|word| word.len() > 2),
        ),
        Value::Array(items) => items.iter().for_each(|item| collect(item, words)),
        Value::Object(fields) => fields.values().for_each(|item| collect(item, words)),
        _ => {}
    }
}
