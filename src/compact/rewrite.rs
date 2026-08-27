use super::model::{Decisions, InputPlan, PlannedInput, RecordDecision};
use crate::ingest::manifest::ManifestEntry;
use crate::kernel::digest::sha256_hex;
use crate::kernel::error::Error;
use std::path::PathBuf;

pub(super) fn planned_input(
    index: usize,
    source: PathBuf,
    before: Vec<u8>,
    entry: &ManifestEntry,
    decisions: &Decisions,
) -> Result<PlannedInput, Error> {
    let text = std::str::from_utf8(&before).map_err(|error| Error::Compact(error.to_string()))?;
    let mut after = Vec::with_capacity(before.len());
    let mut removals = Vec::new();
    let mut retained = Vec::new();
    let mut records_after = 0;
    for (offset, raw) in text.split_inclusive('\n').enumerate() {
        let line = offset + 1;
        match decisions.get(&(index, line)) {
            Some(decision) if decision.remove => removals.push(RecordDecision {
                id: decision.id.clone(),
                reason: decision.reason.expect("removal has a reason"),
            }),
            Some(decision) => {
                records_after += 1;
                write_retained(&mut after, raw, decision.replacement.as_deref());
                if let Some(reason) = decision.reason {
                    retained.push(RecordDecision {
                        id: decision.id.clone(),
                        reason,
                    });
                }
            }
            None => after.extend_from_slice(raw.as_bytes()),
        }
    }
    Ok(PlannedInput {
        declared: entry.path.clone(),
        source,
        before: before.clone(),
        after: after.clone(),
        records_after,
        public: InputPlan {
            path: entry.path.to_string_lossy().into_owned(),
            before_sha256: sha256_hex(&before),
            after_sha256: sha256_hex(&after),
            removals,
            retained,
        },
    })
}

fn write_retained(output: &mut Vec<u8>, raw: &str, replacement: Option<&[u8]>) {
    let Some(replacement) = replacement else {
        output.extend_from_slice(raw.as_bytes());
        return;
    };
    output.extend_from_slice(replacement);
    if raw.ends_with("\r\n") {
        output.extend_from_slice(b"\r\n");
    } else if raw.ends_with('\n') {
        output.push(b'\n');
    }
}
