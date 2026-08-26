use super::{queries, row::ProjectedRow, sqlite};
use crate::kernel::error::Error;
use crate::projection::{ProjectionState, SearchHit, SearchReport, SearchRequest};
use rusqlite::params;
use std::path::Path;

pub fn search(store_root: &Path, request: &SearchRequest) -> Result<SearchReport, Error> {
    let projection_state = sqlite::state(store_root)?;
    if projection_state == ProjectionState::Missing {
        return Err(Error::Projection(
            "sqlite projection is missing; run `equill rebuild --store <path>`".into(),
        ));
    }
    if request.query.trim().is_empty() || !(1..=100).contains(&request.limit) {
        return Err(Error::Projection(
            "search requires a query and a limit between 1 and 100".into(),
        ));
    }
    let connection = sqlite::open(&sqlite::database(store_root))?;
    let query = fts_query(&request.query);
    let mut statement = connection
        .prepare(queries::SEARCH)
        .map_err(|error| sqlite::projection_error("prepare search", error))?;
    let rows = statement
        .query_map(
            params![
                query,
                request.namespace.as_deref(),
                request.type_name.as_deref(),
                i64::from(request.limit)
            ],
            ProjectedRow::read,
        )
        .map_err(|error| sqlite::projection_error("run search", error))?;
    let mut hits = Vec::new();
    for row in rows {
        hits.push(SearchHit {
            record: row
                .map_err(|error| sqlite::projection_error("read search result", error))?
                .record()?,
        });
    }
    Ok(SearchReport {
        ok: projection_state == ProjectionState::Ready,
        projection: "sqlite-fts",
        state: projection_state,
        hits,
    })
}

fn fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND ")
}
