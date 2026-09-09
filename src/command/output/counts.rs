//! The ledger and vector lines of a status report.
use std::fmt::Write as _;

/// The four lines a person reads: what the ledger holds, why the dead numbers
/// do not add up the obvious way, what the index has, and whether anything is
/// running.
pub(super) fn store_counts(output: &mut String, store: &crate::command::status::StoreStatus) {
    if let Some(counts) = &store.counts {
        let _ = write!(
            output,
            "\nRecords: {} total, {} live, {} dead\nDead: {} superseded, {} revoked, {} both",
            counts.ledger_records,
            counts.ledger_live,
            counts.ledger_dead,
            counts.ledger_superseded,
            counts.ledger_revoked,
            counts.ledger_dead_overlap
        );
    }
    let Some(vector) = &store.vector else {
        return;
    };
    let checkpoint = match vector.vector_checkpoint_records {
        Some(records) => format!("{records} at last checkpoint"),
        None => "no usable checkpoint".to_string(),
    };
    let pending = match &vector.vector_pending {
        crate::command::status::pending::Pending::None => "up to date".to_string(),
        crate::command::status::pending::Pending::Records { count } => {
            format!("{count} outside the checkpoint")
        }
        crate::command::status::pending::Pending::Unknown { reason } => {
            format!("unknown ({reason})")
        }
    };
    let _ = write!(
        output,
        "\nVectors: {} eligible, {checkpoint}, pending {pending}\nProcessing: not tracked",
        vector.vector_eligible_records
    );
}
