use crate::ingest::{ImportReport, ImportSetReport};

pub fn import(report: &ImportReport) -> String {
    crate::audit::result::remember(
        report.records.iter().map(|record| record.record_id),
        report.imported,
        None,
        None,
    );
    format!(
        "Imported {} record(s)\nSkipped: {}\nInput SHA-256: {}",
        report.imported, report.skipped, report.input_sha256
    )
}

pub fn import_set(report: &ImportSetReport) -> String {
    crate::audit::result::remember([], report.imported, Some(&report.receipt), None);
    format!(
        "Imported {} record(s) from {} input(s)\nSkipped: {}\nReceipt: {}",
        report.imported, report.inputs, report.skipped, report.receipt
    )
}
