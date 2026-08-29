pub(crate) mod lifecycle;
mod model;
mod receipt;
mod validation;
mod verify;
mod writer;

#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod tests;

pub use model::{AppendReport, EvidenceRef, RecordDraft, StoredRecord};
pub use verify::{read_all, verify_all};
pub use writer::{append, append_file};
