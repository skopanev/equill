//! The vector side of a status report, read without touching the provider.
use super::{VectorCounts, pending};
use crate::kernel::error::Error;
use std::path::Path;

/// Reads the vector side without touching the provider: the corpus comes from
/// the ledger and the checkpoint from the marker file.
pub(super) fn vector_counts(root: &Path) -> Result<Option<VectorCounts>, Error> {
    let Some((corpus, digest)) = crate::vector::status_corpus(root)? else {
        return Ok(None);
    };
    let checkpoint = crate::vector::status_checkpoint(root)?;
    Ok(Some(VectorCounts {
        vector_eligible_records: corpus.len(),
        vector_checkpoint_records: checkpoint.as_ref().map(|(indexed, _)| *indexed),
        vector_pending: pending::assess(
            checkpoint
                .as_ref()
                .map(|(indexed, digest)| (*indexed, digest.as_str())),
            &corpus,
            &digest,
        ),
        vector_processing: "not_tracked",
    }))
}
