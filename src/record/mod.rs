mod model;
mod validation;
mod verify;
mod writer;

#[cfg(test)]
mod tests;

pub use model::{AppendReport, EvidenceRef, RecordDraft, StoredRecord};
pub use verify::verify_all;
pub use writer::{append, append_file};
