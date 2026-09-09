//! The vector side of a status report, read without touching the provider.
use super::{VectorCounts, pending};
use crate::vector::Position;

/// Built from the one reading the whole report shares.
///
/// The eligible count is a ledger property and is given even when no provider
/// is configured: a reader deciding whether to configure one needs to know how
/// much there would be to embed, and hiding the number until the answer is
/// already known helps nobody.
pub(super) fn vector_counts(position: &Position) -> VectorCounts {
    VectorCounts {
        vector_eligible_records: position.corpus.len(),
        vector_checkpoint_records: position.checkpoint.indexed(),
        vector_pending: pending::assess(position),
        vector_processing: "not_tracked",
    }
}
