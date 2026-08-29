use super::super::config::VectorConfig;
use super::super::embedding::EmbeddingRuntime;
use super::super::model::vector_error;
use super::super::progress::{VectorProgress, VectorProgressSink, emit};
use super::super::{VectorProjection, embed_batch};
use super::document::canonical;
use crate::kernel::digest::sha256_hex;
use crate::kernel::error::Error;
use crate::kernel::lock::StoreLock;
use crate::kernel::{identity, store};
use crate::record::StoredRecord;
use serde::Serialize;
use std::fs;
use std::path::Path;
use uuid::Uuid;

const BATCH: usize = 32;

#[derive(Debug, Serialize)]
pub struct VectorRebuildReport {
    pub ok: bool,
    pub projection: &'static str,
    pub collection: String,
    pub records: usize,
    pub corpus_sha256: String,
}

/// Rebuild is staged, then activated. Vectors go into a fresh physical
/// collection while the alias keeps serving the previous one, so a failure
/// anywhere before activation leaves the old answers in place and never marks
/// the projection Ready. Embedding the whole corpus can take minutes, so the
/// ledger is read without the writer lock and re-checked under it: if a record
/// landed while we worked, the digest no longer matches and we refuse to
/// activate a snapshot that is already behind.
pub fn rebuild(store_root: &Path, actor: &str) -> Result<VectorRebuildReport, Error> {
    rebuild_with_progress(store_root, actor, None)
}

pub fn rebuild_with_progress(
    store_root: &Path,
    actor: &str,
    mut progress: Option<&mut dyn VectorProgressSink>,
) -> Result<VectorRebuildReport, Error> {
    let config = store::load(store_root)?;
    identity::require_root(&config, actor)?;
    let vector_config = super::super::config::load(store_root)?
        .filter(|config| config.enabled)
        .ok_or_else(|| vector_error("vector projection is not configured"))?;
    emit(
        &mut progress,
        VectorProgress::Connecting {
            collection: vector_config.collection_alias.clone(),
        },
    );
    let projection = VectorProjection::open(store_root)?
        .ok_or_else(|| vector_error("vector projection is not configured"))?;
    emit(&mut progress, VectorProgress::LoadingModel);
    let embedder = EmbeddingRuntime::load(store_root, &vector_config)?;

    let (records, digest) = corpus(store_root)?;
    let physical = physical_name(&vector_config);
    emit(
        &mut progress,
        VectorProgress::Scanned {
            collection: physical.clone(),
            records: records.len(),
            pending: records.len(),
            corpus_sha256: digest.clone(),
        },
    );
    projection.prepare_collection(&physical)?;
    let mut completed = 0;
    for chunk in records.chunks(BATCH) {
        let documents = chunk
            .iter()
            .map(|(record, digest)| canonical(record, digest))
            .collect::<Result<Vec<_>, _>>()?;
        let points = embed_batch(&embedder, &documents)?;
        completed += points.len();
        emit(
            &mut progress,
            VectorProgress::Embedded {
                completed,
                total: records.len(),
            },
        );
        projection.upsert(&physical, &points)?;
        emit(
            &mut progress,
            VectorProgress::Upserted {
                completed,
                total: records.len(),
            },
        );
    }

    let _lock = StoreLock::exclusive(store_root)?;
    let (_, recheck) = corpus(store_root)?;
    if recheck != digest {
        return Err(vector_error(
            "the ledger changed while embedding; rerun the rebuild",
        ));
    }
    projection.activate(&physical)?;
    drop(_lock);
    emit(
        &mut progress,
        VectorProgress::Ready {
            collection: physical.clone(),
            corpus_sha256: digest.clone(),
        },
    );
    Ok(VectorRebuildReport {
        ok: true,
        projection: "vector-qdrant",
        collection: physical,
        records: records.len(),
        corpus_sha256: digest,
    })
}

/// The ledger is the truth being indexed, so the digest covers exactly what a
/// canonical read returns: every record hash in record-id order.
pub(crate) fn corpus(store_root: &Path) -> Result<(Vec<(StoredRecord, String)>, String), Error> {
    let validated = crate::record::read_all(store_root)?;
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
    Ok((records, sha256_hex(accumulator.as_bytes())))
}

fn physical_name(config: &VectorConfig) -> String {
    format!("{}_{}", config.collection_alias, Uuid::now_v7().simple())
}
