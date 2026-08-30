mod batch;
pub(crate) mod lifecycle;
mod model;
mod receipt;
mod revoke;
mod similar;
mod validation;
mod verify;
mod writer;

#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod tests;

pub use batch::{BatchItem, BatchReport, append_batch, is_batch};
pub use model::{AppendReport, EvidenceRef, RecordDraft, StoredRecord};
pub use revoke::{REVOKED_TAG, RevokeReport, revoke};
pub use similar::{SimilarRecord, find as find_similar};
pub use verify::{read_all, verify_all};
#[cfg(test)]
pub(crate) use writer::require_current_writer;
pub use writer::{append, append_file, append_only};
