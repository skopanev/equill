use super::{AppendReport, RecordDraft, append_only};
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
    let mut records = Vec::new();
    let mut stored = 0;
    let mut rejected = 0;
    for (index, line) in contents.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let line_number = index + 1;
        match write(store_root, line, actor) {
            Ok(report) => {
                stored += 1;
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
        crate::vector::after_commit(store_root, stored as u64);
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

/// The number of records in the file decides the shape of the answer, so a
/// single-record file keeps behaving exactly as it did.
pub fn is_batch(source: &Path) -> Result<bool, Error> {
    let contents = fs::read_to_string(source)?;
    Ok(contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count()
        > 1)
}

fn write(store_root: &Path, line: &str, actor: &str) -> Result<AppendReport, Error> {
    let draft: RecordDraft = serde_json::from_str(line)?;
    append_only(store_root, draft, actor)
}

#[cfg(test)]
mod tests;
