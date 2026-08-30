use super::doctor::DoctorReport;
use super::init::InitReport;
use super::status::StatusReport;
use crate::compact::CompactReport;
use crate::context::RegistryReport;
use crate::ingest::{ImportReport, ImportSetReport};
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

pub fn import_set(report: &ImportSetReport) -> String {
    format!(
        "Imported {} record(s) from {} input(s)\nSkipped: {}\nReceipt: {}",
        report.imported, report.inputs, report.skipped, report.receipt
    )
}

pub fn compact(report: &CompactReport) -> String {
    let mode = if report.applied { "Applied" } else { "Dry run" };
    let mut output = format!(
        "{mode}: {} removal(s) across {} input(s)",
        report.removed,
        report.inputs.len()
    );
    for input in &report.inputs {
        write!(
            &mut output,
            "\n  {}: {} remove, {} retained with reason",
            input.path,
            input.removals.len(),
            input.retained.len()
        )
        .expect("writing to String cannot fail");
        for item in &input.removals {
            write!(&mut output, "\n    remove {} ({})", item.id, item.reason)
                .expect("writing to String cannot fail");
        }
        for item in &input.retained {
            write!(&mut output, "\n    retain {} ({})", item.id, item.reason)
                .expect("writing to String cannot fail");
        }
    }
    if let Some(receipt) = &report.receipt {
        write!(&mut output, "\nReceipt: {receipt}").expect("writing to String cannot fail");
    }
    output
}

pub fn schema(report: &RegisterReport) -> String {
    let action = if report.created {
        "Registered"
    } else {
        "Already registered"
    };
    format!("{action}: {}\nSHA-256: {}", report.type_name, report.sha256)
}

pub fn registry(kind: &str, report: &RegistryReport) -> String {
    let action = if report.created {
        "Registered"
    } else {
        "Already registered"
    };
    format!(
        "{action} {kind}: {}@{}\nSHA-256: {}",
        report.id, report.version, report.digest
    )
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
        // `ready` on the left already says the component works. What a reader
        // still needs is whether it has caught up, and only when it has not.
        if let Some(pending) = component
            .vector
            .as_ref()
            .and_then(|health| health.vector_pending_records)
            .filter(|pending| *pending > 0)
        {
            write!(&mut output, " — {pending} processing").expect("writing to String cannot fail");
        }
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

pub fn authority(report: &crate::governance::AuthorityReport) -> String {
    let grants = report
        .grants
        .iter()
        .map(|grant| {
            format!(
                "  {} -> {} {}",
                grant.actors.join(","),
                grant.namespace,
                grant.types.join(",")
            )
        })
        .collect::<Vec<_>>();
    let mut text = format!(
        "owner {}\nwriters {}",
        report.owner,
        if report.writers.is_empty() {
            "none".into()
        } else {
            report.writers.join(", ")
        }
    );
    if !grants.is_empty() {
        text.push_str("\ngrants\n");
        text.push_str(&grants.join("\n"));
    }
    text
}

pub fn owner(report: &crate::governance::OwnerReport) -> String {
    let revoked = if report.revoked_writers.is_empty() {
        String::new()
    } else {
        format!(
            " — {} lost {}",
            report.previous_owner,
            report.revoked_writers.join(" and ")
        )
    };
    format!(
        "{} handed the store to {}{}",
        report.previous_owner, report.owner, revoked
    )
}

pub fn grant(report: &crate::governance::GrantReport) -> String {
    if report.changed {
        format!("{} — {} grants in force", report.actor, report.grants)
    } else {
        format!(
            "{} — unchanged, {} grants in force",
            report.actor, report.grants
        )
    }
}
