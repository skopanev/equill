use super::super::Embedder;
use super::super::config::VectorConfig;
use super::super::model::{EmbeddingDocument, VectorPointMetadata, vector_error};
use super::document::canonical;
use super::index::SyncIndex;
use crate::kernel::error::Error;
use crate::record::StoredRecord;
use std::collections::{HashMap, HashSet};

const SCAN_BATCH: usize = 256;

/// What has to be embedded, and what only has to be re-labelled.
///
/// A record whose envelope changed but whose meaning did not — compaction cuts
/// a `supersedes` link, and `record_sha256` moves with it — has the same
/// embedding input as before, because the input carries no provenance. Sending
/// it back to the model would spend the whole corpus to relabel a field the
/// model never saw. The point keeps its vector and takes the new hash.
pub(super) struct Work {
    pub embed: Vec<EmbeddingDocument>,
    pub relabel: Vec<EmbeddingDocument>,
}

pub(super) fn pending<I: SyncIndex>(
    config: &VectorConfig,
    index: &I,
    physical: &str,
    records: &[(StoredRecord, String)],
) -> Result<Work, Error> {
    let mut pending = Work {
        embed: Vec::new(),
        relabel: Vec::new(),
    };
    for chunk in records.chunks(SCAN_BATCH) {
        let ids = chunk
            .iter()
            .map(|(record, _)| record.id)
            .collect::<Vec<_>>();
        let requested = ids.iter().copied().collect::<HashSet<_>>();
        let mut current = HashMap::<_, VectorPointMetadata>::new();
        for item in index.metadata(physical, &ids)? {
            if !requested.contains(&item.record_id)
                || current.insert(item.record_id, item).is_some()
            {
                return Err(vector_error("retrieval returned unexpected point metadata"));
            }
        }
        for (record, record_sha256) in chunk {
            let document = canonical(record, record_sha256, config.max_document_chars)?;
            let known = current.get(&record.id);
            let same_meaning = known.is_some_and(|item| {
                item.input_sha256 == document.input_sha256
                    && item.model_sha256 == config.embedding.model_sha256()
            });
            let same_record =
                known.is_some_and(|item| item.record_sha256 == document.record_sha256);
            match (same_meaning, same_record) {
                (true, true) => {}
                (true, false) => pending.relabel.push(document),
                _ => pending.embed.push(document),
            }
        }
    }
    Ok(pending)
}

pub(super) fn verify_descriptor(
    config: &VectorConfig,
    embedder: &impl Embedder,
) -> Result<(), Error> {
    let descriptor = embedder.descriptor();
    if descriptor.model_id != config.embedding.model_id()
        || descriptor.model_sha256 != config.embedding.model_sha256()
        || descriptor.tokenizer_sha256 != config.embedding.tokenizer_sha256()
        || descriptor.dimensions != config.dimensions
        || descriptor.distance != config.distance
        || descriptor.input_schema != config.embedding.input_schema()
    {
        return Err(vector_error("embedder does not match vector configuration"));
    }
    Ok(())
}
