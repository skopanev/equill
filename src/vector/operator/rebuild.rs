use super::super::config::VectorConfig;
use super::super::embedding::EmbeddingRuntime;
use super::super::model::vector_error;
use super::super::progress::{VectorProgress, VectorProgressSink, emit};
use super::super::{VectorProjection, embed_batch};
use super::document::canonical;
use crate::kernel::error::Error;
use crate::kernel::governance::RootGuard;
use crate::kernel::lock::StoreLock;

use crate::record::StoredRecord;
use serde::Serialize;
use std::path::Path;
use uuid::Uuid;

const BATCH: usize = 32;

#[derive(Debug, Serialize)]
pub struct VectorRebuildReport {
    pub ok: bool,
    pub projection: &'static str,
    pub collection: String,
    pub records: usize,
    /// Live records `embed_types` left out of this pass. Reported so the saving
    /// is visible rather than inferred from a number that got smaller.
    pub records_skipped: usize,
    pub corpus_sha256: String,
}

/// Rebuild is staged, then activated. Vectors go into a fresh physical
/// collection while the alias keeps serving the previous one, so a failure
/// anywhere before activation leaves the old answers in place.
///
/// It indexes the snapshot it captured and activates exactly that boundary. It
/// does not require the ledger to hold still: a store that is written to
/// continuously would never satisfy such a condition, and refusing to activate
/// would leave it with no index at all. Whatever arrives during the pass is the
/// next sync's tail, and the checkpoint records where this one stopped.
pub fn rebuild(store_root: &Path, actor: &str) -> Result<VectorRebuildReport, Error> {
    rebuild_with_progress(store_root, actor, None)
}

pub fn rebuild_with_progress(
    store_root: &Path,
    actor: &str,
    mut progress: Option<&mut dyn VectorProgressSink>,
) -> Result<VectorRebuildReport, Error> {
    let (_guard, _config) = RootGuard::acquire(store_root, actor)?;
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

    let Captured {
        records,
        digest,
        revision,
        skipped_by_type: skipped,
        embed_types_sha256,
        history: _,
    } = capture(store_root)?;
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
    // The revision activated is the one captured with the corpus. Reading the
    // target here instead claimed everything written during the pass as
    // covered: those records were never embedded, the checkpoint drew level
    // with the target, and the gate — which compares exactly those two numbers
    // — saw nothing outstanding. The tail was lost until a hand-run sync.
    projection.activate(
        &physical,
        Some((
            records.len(),
            &digest,
            revision,
            embed_types_sha256.as_deref(),
        )),
    )?;
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
        records_skipped: skipped,
        corpus_sha256: digest,
    })
}

/// What a rebuild indexes, and the target that snapshot covers.
pub(crate) struct Captured {
    pub(crate) records: Vec<(StoredRecord, String)>,
    pub(crate) digest: String,
    pub(crate) revision: u64,
    pub(crate) history: Vec<Uuid>,
    /// Live records the filter left out. Carried with the rest of the boundary
    /// rather than counted again: a second read could disagree with the one
    /// that was indexed.
    pub(crate) skipped_by_type: usize,
    /// The filter the corpus above was taken under.
    pub(crate) embed_types_sha256: Option<String>,
}

/// Both halves of the boundary, taken together.
///
/// Under one writer lock so an append cannot land between them: a corpus from
/// before a write and a target from after it describe a pass that covered
/// something it never saw. The lock is released before embedding — holding it
/// for a model run would stop writes, the very thing this contract avoids — so
/// whatever arrives afterwards is the next pass's tail, and the checkpoint says
/// so by staying behind the target. Target first, then the corpus, matching the
/// incremental sync.
pub(crate) fn capture(store_root: &Path) -> Result<Captured, Error> {
    let (revision, embed_types, captured) = {
        let _lock = StoreLock::exclusive(store_root)?;
        let revision =
            crate::vector::desired::read(store_root)?.map_or(0, |target| target.revision);
        let embed_types = super::super::coverage::embed_types(store_root)?;
        let captured = crate::record::snapshot::capture_exclusive(store_root, None)?;
        (revision, embed_types, captured)
    };
    // Descriptors, lengths, target and filter are fixed. Hashing and validation
    // happen after releasing the writer lock, just like embedding and indexing.
    let snapshot = super::super::coverage::from_snapshot(
        crate::record::read_captured(store_root, captured)?,
        embed_types,
    )?;
    Ok(Captured {
        records: snapshot.records,
        digest: snapshot.digest,
        revision,
        skipped_by_type: snapshot.skipped_by_type,
        history: snapshot.history,
        embed_types_sha256: snapshot.embed_types_sha256,
    })
}

fn physical_name(config: &VectorConfig) -> String {
    format!("{}_{}", config.collection_alias, Uuid::now_v7().simple())
}
