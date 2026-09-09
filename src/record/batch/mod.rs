use super::{AppendReport, AppendRequest, RecordDraft, append_only_request};
use crate::kernel::error::Error;
use serde::Serialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct BatchReport {
    pub ok: bool,
    pub stored: usize,
    pub rejected: usize,
    pub records: Vec<BatchItem>,
}

#[derive(Debug, Serialize)]
pub struct BatchItem {
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<uuid::Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Loading a batch used to mean one invocation per record, each re-opening the
/// store. Records are validated and written one at a time, and a rejected line
/// stops only itself: a partial load with a per-line reason is more useful than
/// refusing forty records because one of them is malformed. Every accepted line
/// went through the same immutable writer as a single `record` call.
pub fn append_batch(store_root: &Path, source: &Path, actor: &str) -> Result<BatchReport, Error> {
    let contents = fs::read_to_string(source)?;
    if is_legacy_import(&contents) {
        return crate::ingest::import_jsonl(store_root, source, actor).map(|report| BatchReport {
            ok: report.ok,
            stored: report.imported,
            rejected: 0,
            records: report
                .records
                .into_iter()
                .map(|item| BatchItem {
                    line: item.line,
                    id: Some(item.record_id),
                    error: None,
                })
                .collect(),
        });
    }
    let mut records = Vec::new();
    let mut stored = 0;
    let mut rejected = 0;
    let mut unkeyed = 0;
    for (index, line) in contents.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let line_number = index + 1;
        match write(store_root, line, actor) {
            Ok((report, advance)) => {
                stored += 1;
                unkeyed += u64::from(advance);
                records.push(BatchItem {
                    line: line_number,
                    id: Some(report.id),
                    error: None,
                });
            }
            Err(error) => {
                rejected += 1;
                records.push(BatchItem {
                    line: line_number,
                    id: None,
                    error: Some(error.to_string()),
                });
            }
        }
    }
    if stored > 0 {
        // One catch-up for the whole batch rather than one per record: forty
        // records should cost one pass, not forty.
        let _ = crate::projection::catch_up_text(store_root);
        crate::vector::after_commit(store_root, unkeyed);
    }
    if records.is_empty() {
        return Err(Error::InvalidRecord("input contains no records".into()));
    }
    Ok(BatchReport {
        ok: rejected == 0,
        stored,
        rejected,
        records,
    })
}

fn is_legacy_import(contents: &str) -> bool {
    contents
        .lines()
        .find(|line| !line.trim().is_empty())
        .and_then(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .is_some_and(|value| {
            value.get("id").is_some() && value.get("ts").is_some() && value.get("actor").is_some()
        })
}

/// The number of records in the file decides the shape of the answer, so a
/// single-record file keeps behaving exactly as it did.
pub fn is_batch(source: &Path) -> Result<bool, Error> {
    let contents = fs::read_to_string(source)?;
    if serde_json::from_str::<RecordDraft>(&contents).is_ok()
        || serde_json::from_str::<AppendRequest>(&contents).is_ok()
    {
        return Ok(false);
    }
    let mut lines = 0;
    let mut object_line = false;
    for line in contents.lines().filter(|line| !line.trim().is_empty()) {
        lines += 1;
        object_line |=
            serde_json::from_str::<serde_json::Value>(line).is_ok_and(|value| value.is_object());
    }
    Ok(lines > 1 && object_line)
}

fn write(store_root: &Path, line: &str, actor: &str) -> Result<(AppendReport, bool), Error> {
    let value: serde_json::Value = serde_json::from_str(line)?;
    let request = if value.get("draft").is_some() {
        serde_json::from_value(value)?
    } else {
        AppendRequest {
            draft: serde_json::from_value(value)?,
            idempotency_key: None,
        }
    };
    let advance = request.idempotency_key.is_none();
    append_only_request(store_root, request, actor).map(|report| (report, advance))
}

#[cfg(test)]
mod tests;
