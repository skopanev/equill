//! Which records a later one replaced, and which a tombstone withdrew.
//!
//! Both facts are columns written when the record that establishes them is
//! indexed, so answering is an index lookup rather than a walk. Before this,
//! every semantic search read the whole ledger twice to ask the same two
//! questions.
use super::{queries, sqlite};
use crate::kernel::error::Error;
use crate::projection::{HistoricRecords, HistoryCount, LifecycleScope};
use rusqlite::{params, params_from_iter};
use std::collections::HashSet;
use std::path::Path;
use uuid::Uuid;

/// Bound on one statement's parameters. SQLite accepts far more, but a query
/// built from an unbounded caller-supplied list is a query whose shape the
/// caller decides.
const CHUNK: usize = 256;

pub fn historic(store_root: &Path, ids: &[Uuid]) -> Result<HistoricRecords, Error> {
    let state = sqlite::state(store_root)?;
    let mut history = HashSet::new();
    if ids.is_empty() {
        return Ok(HistoricRecords { state, history });
    }
    let connection = sqlite::open(&sqlite::database(store_root))?;
    for chunk in ids.chunks(CHUNK) {
        let placeholders = (1..=chunk.len())
            .map(|position| format!("?{position}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT id FROM records WHERE id IN ({placeholders}) \
             AND (superseded = 1 OR revoked = 1)"
        );
        let mut statement = connection
            .prepare(&sql)
            .map_err(|error| sqlite::projection_error("prepare lifecycle lookup", error))?;
        let rows = statement
            .query_map(params_from_iter(chunk.iter().map(Uuid::to_string)), |row| {
                row.get::<_, String>(0)
            })
            .map_err(|error| sqlite::projection_error("run lifecycle lookup", error))?;
        for row in rows {
            let id = row.map_err(|error| sqlite::projection_error("read lifecycle row", error))?;
            history
                .insert(Uuid::parse_str(&id).map_err(|error| {
                    Error::Projection(format!("invalid projected id: {error}"))
                })?);
        }
    }
    Ok(HistoricRecords { state, history })
}

pub fn history_in_scope(store_root: &Path, scope: &LifecycleScope) -> Result<HistoryCount, Error> {
    let state = sqlite::state(store_root)?;
    let connection = sqlite::open(&sqlite::database(store_root))?;
    let history = connection
        .query_row(
            queries::HISTORY_IN_SCOPE,
            params![scope.namespace.as_deref(), scope.type_name.as_deref()],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| sqlite::projection_error("count history in scope", error))?;
    Ok(HistoryCount {
        state,
        history: history as usize,
    })
}
