use super::doctor::DoctorReport;
use super::init::InitReport;
use super::status::StatusReport;
use crate::compact::CompactReport;
use crate::context::RegistryReport;
use crate::kernel::error::Error;
use crate::projection::{RebuildReport, SearchReport};
use crate::record::AppendReport;
use crate::schema::RegisterReport;
use serde::Serialize;
use std::fmt::Write;
use std::path::Path;
mod authority;
mod counts;
mod ingest;

pub use authority::{authority, grant, owner, reader};
pub use ingest::{import, import_set};

pub fn render<T: Serialize>(json: bool, value: &T, human: String) -> Result<String, Error> {
    if json {
        Ok(serde_json::to_string(value)?)
    } else {
        Ok(human)
    }
}

/// What a native compaction did, or would do. The counts lead, because the
/// question a reader is asking is how much is going away — and the rewritten
/// envelopes are named separately, since those records survive with a
/// different hash than they had.
pub fn native_compact(report: &crate::compact::NativeReport) -> String {
    let verb = if report.applied {
        "Removed"
    } else {
        "Would remove"
    };
    let mut text = format!(
        "{verb} {} of {} records, keeping {}",
        report.removed,
        report.removed + report.retained,
        report.retained
    );
    if report.severed > 0 {
        let _ = write!(
            text,
            "\n{} retained records lose a link into the removed past and change hash",
            report.severed
        );
    }
    if report.expired_idempotency_keys > 0 {
        let _ = write!(
            text,
            "\n{} idempotency key(s) expire with physically removed records; later requests using those keys are new writes",
            report.expired_idempotency_keys
        );
    }
    if !report.applied {
        text.push_str("\nNothing was changed. Re-run with --apply.");
    }
    text
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
    crate::audit::result::remember([report.id], 1, Some(&report.receipt), Some(report.durable));
    // Two projections, named: a bare "Projection: ready" described the text
    // index while reading as a claim about search freshness generally.
    format!(
        "Recorded {}\nLedger: {}\nReceipt: {}\nText index: {}\nVector index: {}",
        report.id,
        report.ledger,
        report.receipt,
        report.projection,
        super::vector_state::vector_state(report)
    )
}

pub fn batch(report: &crate::record::BatchReport) -> String {
    crate::audit::result::remember(
        report.records.iter().filter_map(|item| item.id),
        report.stored,
        None,
        None,
    );
    format!("{} stored, {} rejected", report.stored, report.rejected)
}

pub fn revoke(report: &crate::record::RevokeReport) -> String {
    crate::audit::result::remember([report.tombstone], 1, Some(&report.receipt), Some(true));
    format!(
        "Revoked {} — tombstone {}",
        report.revoked, report.tombstone
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
            counts::store_counts(&mut output, store);
        }
    }
    output.push_str("\nComponents:");
    for component in &report.components {
        write!(&mut output, "\n  {:<10} {}", component.state, component.id)
            .expect("writing to String cannot fail");
        // `ready` on the left already says the component works. What a reader
        // still needs is whether it has caught up, and only when it has not.
        // Not "processing": nothing here watches a running pass, and the number
        // is the backlog the last checkpoint left behind.
        if let Some(pending) = component
            .vector
            .as_ref()
            .and_then(|health| health.vector_pending_records)
            .filter(|pending| *pending > 0)
        {
            write!(&mut output, " — {pending} outside the checkpoint")
                .expect("writing to String cannot fail");
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
    crate::audit::result::remember([], report.records, None, None);
    format!(
        "Rebuilt {}\nRecords indexed: {}",
        report.projection, report.records
    )
}
