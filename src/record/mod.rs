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
mod boundary_tests;
#[cfg(test)]
mod failure_tests;
#[cfg(test)]
pub(crate) mod hotpath;
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

/// Append, then bring the text index level before returning.
///
/// For tests that search for what they just wrote. Production callers no longer
/// get a synchronous index — the worker does it — so a test that asserts on
/// search results is otherwise racing a background process. Making that wait
/// explicit is the honest form: the test says it needs the index caught up,
/// rather than relying on the write path to do it as a side effect.
#[cfg(test)]
pub(crate) fn append_indexed(
    store_root: &std::path::Path,
    draft: RecordDraft,
    actor: &str,
) -> Result<AppendReport, crate::kernel::error::Error> {
    let report = append(store_root, draft, actor)?;
    let _ = crate::projection::catch_up_text(store_root);
    Ok(report)
}
