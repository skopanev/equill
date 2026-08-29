use super::super::Embedder;
use super::super::config::VectorConfig;
use super::super::model::{EmbeddingDocument, VectorPointMetadata, vector_error};
use super::document::canonical;
use super::rebuild::corpus;
use super::sync::SyncIndex;
use crate::kernel::error::Error;
use crate::record::StoredRecord;
use std::collections::{HashMap, HashSet};

const SCAN_BATCH: usize = 256;

pub(super) fn require_unchanged(store_root: &std::path::Path, expected: &str) -> Result<(), Error> {
    let (_, recheck) = corpus(store_root)?;
    if recheck != expected {
        return Err(vector_error(
            "the ledger changed during vector sync; rerun vector sync",
        ));
    }
    Ok(())
}

pub(super) fn pending<I: SyncIndex>(
    config: &VectorConfig,
    index: &I,
    physical: &str,
    records: &[(StoredRecord, String)],
) -> Result<Vec<EmbeddingDocument>, Error> {
    let mut pending = Vec::new();
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
            let document = canonical(record, record_sha256)?;
            let compatible = current.get(&record.id).is_some_and(|item| {
                item.record_sha256 == document.record_sha256
                    && item.input_sha256 == document.input_sha256
                    && item.model_sha256 == config.embedding.model.sha256
            });
            if !compatible {
                pending.push(document);
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
    if descriptor.model_id != config.embedding.model_id
        || descriptor.model_sha256 != config.embedding.model.sha256
        || descriptor.tokenizer_sha256 != config.embedding.tokenizer.sha256
        || descriptor.dimensions != config.dimensions
        || descriptor.distance != config.distance
        || descriptor.input_schema != config.embedding.input_schema
    {
        return Err(vector_error("embedder does not match vector configuration"));
    }
    Ok(())
}
