mod batch;
pub(crate) mod lifecycle;
mod model;
mod receipt;
mod similar;
mod validation;
mod verify;
mod writer;

#[cfg(test)]
mod batch_tests;
#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod tests;

pub use batch::{BatchItem, BatchReport, append_batch, is_batch};
pub use model::{AppendReport, EvidenceRef, RecordDraft, StoredRecord};
pub use similar::{SimilarRecord, find as find_similar};
pub use verify::{read_all, verify_all};
pub use writer::{append, append_file};
