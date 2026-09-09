use super::sqlite::{failure, ready};
use crate::audit::{Event, Scope, Statistics};
use crate::kernel::error::Error;
use rusqlite::{params_from_iter, types::Value};
use std::path::Path;

fn predicate(scope: &Scope) -> Result<(String, Vec<Value>), Error> {
    let mut terms = Vec::new();
    let mut values = Vec::new();
    for (column, value) in [
        ("surface", &scope.surface),
        ("operation", &scope.operation),
        ("project", &scope.project),
        ("role", &scope.role),
        ("process", &scope.process),
        ("outcome", &scope.outcome),
        ("instance", &scope.instance),
        ("session", &scope.session),
        ("actor", &scope.actor),
        ("lane", &scope.lane),
    ] {
        if let Some(value) = value {
            terms.push(format!("{column}=?"));
            values.push(Value::Text(value.clone()));
        }
    }
    for (comparison, value) in [(">=", &scope.since), ("<", &scope.until)] {
        if let Some(value) = value {
            let at = value.parse::<jiff::Timestamp>().map_err(failure)?;
            terms.push(format!("(at,nanos){comparison}(?,?)"));
            values.push(Value::Integer(at.as_second()));
            values.push(Value::Integer(i64::from(at.subsec_nanosecond())));
        }
    }
    let clause = if terms.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", terms.join(" AND "))
    };
    Ok((clause, values))
}

pub(crate) fn list(root: &Path, scope: &Scope, limit: u16) -> Result<Vec<Event>, Error> {
    let (connection, _index_lock) = ready(root)?;
    let (clause, mut values) = predicate(scope)?;
    values.push(Value::Integer(i64::from(limit)));
    let mut statement = connection
        .prepare(&format!(
            "SELECT event FROM events{clause} ORDER BY at DESC, nanos DESC, id DESC LIMIT ?"
        ))
        .map_err(failure)?;
    let rows = statement
        .query_map(params_from_iter(values), |row| row.get::<_, String>(0))
        .map_err(failure)?;
    rows.map(|row| serde_json::from_str(&row.map_err(failure)?).map_err(failure))
        .collect()
}

pub(crate) fn stats(root: &Path, scope: &Scope) -> Result<Statistics, Error> {
    let (connection, _index_lock) = ready(root)?;
    let (clause, values) = predicate(scope)?;
    let sql = format!(
        "SELECT count(*),coalesce(sum(outcome='error'),0),coalesce(min(duration),0),coalesce(max(duration),0),coalesce(avg(duration),0) FROM events{clause}"
    );
    let mut stats = connection
        .query_row(&sql, params_from_iter(&values), |row| {
            Ok(Statistics {
                count: unsigned(row, 0)?,
                failures: unsigned(row, 1)?,
                duration_min_us: unsigned(row, 2)?,
                duration_max_us: unsigned(row, 3)?,
                duration_mean_us: row.get(4)?,
                ..Statistics::default()
            })
        })
        .map_err(failure)?;
    if stats.count > 0 {
        stats.error_rate = stats.failures as f64 / stats.count as f64;
        let offset = (stats.count * 95).div_ceil(100) - 1;
        let mut values = values;
        values.push(Value::Integer(offset as i64));
        stats.duration_p95_us = connection
            .query_row(
                &format!("SELECT duration FROM events{clause} ORDER BY duration LIMIT 1 OFFSET ?"),
                params_from_iter(values),
                |row| unsigned(row, 0),
            )
            .map_err(failure)?;
    }
    Ok(stats)
}

fn unsigned(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(column)?;
    value
        .try_into()
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(column, value))
}
