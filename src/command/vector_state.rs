//! How a write reports the vector index, in the human-readable form.
use crate::record::AppendReport;

pub(super) fn vector_state(report: &AppendReport) -> String {
    let state = match report.vector.projection {
        crate::vector::Projection::Current => "current",
        crate::vector::Projection::Queued => "queued",
        crate::vector::Projection::Disabled => "disabled",
        crate::vector::Projection::NotApplicable => "not applicable",
    };
    match report.vector.attempt_error {
        Some(_) => format!("{state} (last attempt failed)"),
        None => state.to_owned(),
    }
}
