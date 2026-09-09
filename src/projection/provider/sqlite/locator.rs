//! Read only coordinates and digests, never the projected payload.
use super::sqlite::{database, open, state};
use crate::kernel::error::Error;
use crate::projection::{LedgerLocator, LocatorReport, LocatorRequest, ProjectionState};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub fn locators(root: &Path, request: &LocatorRequest) -> Result<LocatorReport, Error> {
    let mut seen = HashSet::new();
    if request
        .ids
        .iter()
        .any(|id| id.get_version() != Some(uuid::Version::SortRand) || !seen.insert(*id))
    {
        return Err(failure());
    }
    let mut report = LocatorReport {
        state: state(root).map_err(|_| failure())?,
        watermark: crate::projection::watermark(root),
        located: Vec::new(),
    };
    if request.ids.is_empty() || report.state == ProjectionState::Missing {
        return Ok(report);
    }
    let mut connection = open(&database(root)).map_err(|_| failure())?;
    let transaction = connection.transaction().map_err(|_| failure())?;
    let mut found = HashMap::new();
    for chunk in request.ids.chunks(256) {
        let slots = vec!["?"; chunk.len()].join(",");
        let mut select = transaction
            .prepare(&format!(
                "SELECT id, ledger, record_sha256 FROM records WHERE id IN ({slots})"
            ))
            .map_err(|_| failure())?;
        let rows = select
            .query_map(
                rusqlite::params_from_iter(chunk.iter().map(ToString::to_string)),
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .map_err(|_| failure())?;
        for row in rows {
            let (id, ledger, record_sha256) = row.map_err(|_| failure())?;
            let record_id = id.parse().map_err(|_| failure())?;
            if found
                .insert(
                    record_id,
                    LedgerLocator {
                        record_id,
                        ledger,
                        record_sha256,
                    },
                )
                .is_some()
            {
                return Err(failure());
            }
        }
    }
    report.located = request
        .ids
        .iter()
        .filter_map(|id| found.remove(id))
        .collect();
    Ok(report)
}

#[cfg(test)]
mod tests;

fn failure() -> Error {
    Error::Projection("candidate ledger locators are unavailable or invalid".into())
}
