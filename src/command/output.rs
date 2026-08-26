use super::doctor::DoctorReport;
use super::init::InitReport;
use super::status::StatusReport;
use crate::ingest::ImportReport;
use crate::kernel::error::Error;
use crate::projection::{RebuildReport, SearchReport};
use crate::record::AppendReport;
use crate::schema::RegisterReport;
use serde::Serialize;
use std::fmt::Write;
use std::path::Path;

pub fn render<T: Serialize>(json: bool, value: &T, human: String) -> Result<String, Error> {
    if json {
        Ok(serde_json::to_string(value)?)
    } else {
        Ok(human)
    }
}

pub fn init(path: &Path, report: &InitReport) -> String {
    if report.created {
        format!(
            "Initialized {}\nProjection: sqlite-fts ready",
            path.display()
        )
    } else {
        format!("Already initialized: {}", path.display())
    }
}

pub fn record(report: &AppendReport) -> String {
    format!(
        "Recorded {}\nLedger: {}\nReceipt: {}\nProjection: {}",
        report.id, report.ledger, report.receipt, report.projection
    )
}

pub fn import(report: &ImportReport) -> String {
    format!(
        "Imported {} record(s)\nSkipped: {}\nInput SHA-256: {}",
        report.imported, report.skipped, report.input_sha256
    )
}

pub fn schema(report: &RegisterReport) -> String {
    let action = if report.created {
        "Registered"
    } else {
        "Already registered"
    };
    format!("{action}: {}\nSHA-256: {}", report.type_name, report.sha256)
}

pub fn doctor(report: &DoctorReport) -> String {
    let state = if report.ok { "OK" } else { "ATTENTION" };
    let mut output = format!("Equill doctor ({}) — {state}", report.mode);
    for check in &report.checks {
        write!(&mut output, "\n  {:<24} {}", check.id, check.items)
            .expect("writing to String cannot fail");
    }
    if let Some(deep) = &report.deep_defense {
        write!(
            &mut output,
            "\n  {:<24} {} finding(s)\nReceipt: {}",
            "deep-memory-defense", deep.findings, deep.receipt
        )
        .expect("writing to String cannot fail");
    }
    output
}

pub fn status(report: &StatusReport) -> String {
    let mut output = format!("Equill {}", report.version);
    match &report.store {
        None => output.push_str("\nStore: not selected"),
        Some(store) if !store.initialized => output.push_str("\nStore: not initialized"),
        Some(store) => {
            write!(
                &mut output,
                "\nStore: ready\nNamespaces: {}\nSchemas: {}",
                store.namespaces.len(),
                store.schemas.len()
            )
            .expect("writing to String cannot fail");
        }
    }
    output.push_str("\nComponents:");
    for component in &report.components {
        write!(&mut output, "\n  {:<10} {}", component.state, component.id)
            .expect("writing to String cannot fail");
    }
    output
}

pub fn search(report: &SearchReport) -> String {
    if report.hits.is_empty() {
        return "No matches.".into();
    }
    let mut output = format!("{} match(es)", report.hits.len());
    for hit in &report.hits {
        write!(
            &mut output,
            "\n\n{}  {}  {}\n{}",
            hit.record.id, hit.record.type_name, hit.record.observed_at, hit.record.payload
        )
        .expect("writing to String cannot fail");
    }
    output
}

pub fn rebuild(report: &RebuildReport) -> String {
    format!(
        "Rebuilt {}\nRecords indexed: {}",
        report.projection, report.records
    )
}
