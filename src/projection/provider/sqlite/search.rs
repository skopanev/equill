use super::{queries, row::ProjectedRow, sqlite};
use crate::kernel::error::Error;
use crate::projection::{ProjectionState, SearchHit, SearchReport, SearchRequest};
use rusqlite::params;
use std::path::Path;

/// The largest candidate pool one search may scan. A filter needs to look past
/// the caller's page size, but not without a bound.
pub const MAX_SCAN: u16 = 10_000;

pub fn search(store_root: &Path, request: &SearchRequest) -> Result<SearchReport, Error> {
    let projection_state = sqlite::state(store_root)?;
    if projection_state == ProjectionState::Missing {
        return Err(Error::Projection(
            "sqlite projection is missing; run `equill rebuild --store <path>`".into(),
        ));
    }
    // The page a caller asks for and the pool a filter has to look through are
    // different numbers. Conflating them made an ordinary filtered search fail
    // with a message about a limit the caller never set.
    // An absent query means the caller is selecting by filter alone: every
    // record in scope is a candidate, and the filter decides.
    let query = request.query.as_deref().map(str::trim).unwrap_or_default();
    if request.limit == 0 {
        return Err(Error::Projection(
            "search requires a limit above zero".into(),
        ));
    }
    if request.limit > MAX_SCAN {
        return Err(Error::Projection(format!(
            "search can scan at most {MAX_SCAN} candidates; narrow the namespace or type"
        )));
    }
    let connection = sqlite::open(&sqlite::database(store_root))?;
    let query = if query.is_empty() {
        String::new()
    } else {
        fts_query(query)
    };
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

/// Candidate retrieval is an OR over the query terms: one hit is enough to
/// surface a record. Every term stays a quoted literal, so a user-typed quote
/// cannot open a phrase or an operator, and bm25 still orders the top-k rows —
/// a record matching more terms keeps ranking above one matching fewer.
fn fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" OR ")
}

#[cfg(test)]
mod tests {
    use super::fts_query;
    use crate::command::init;
    use crate::projection::{self, SearchRequest};
    use crate::record::{RecordDraft, append};
    use crate::schema::{self, TypeDefinition};
    use serde_json::json;
    use std::fs;
    use std::path::{Path, PathBuf};
    use uuid::Uuid;

    fn store() -> PathBuf {
        let root = std::env::temp_dir().join(format!("equill-fts-{}", Uuid::now_v7()));
        init::create(&root, "writer", "agent.memory").expect("initialize");
        schema::register(
            &root,
            TypeDefinition {
                type_name: "agent.lesson.v1".into(),
                uri: "equill://agent.lesson/v1".into(),
                owner: "writer".into(),
                payload_schema: json!({
                    "$schema": "https://json-schema.org/draft/2020-12/schema",
                    "type": "object",
                    "properties": { "rule": { "type": "string" } },
                    "required": ["rule"],
                    "additionalProperties": false
                }),
                lifecycle: Default::default(),
            },
            "writer",
        )
        .expect("register schema");
        for rule in ["Deployment checklist for staging", "Unrelated grocery list"] {
            append(
                &root,
                RecordDraft {
                    namespace: "agent.memory".into(),
                    type_name: "agent.lesson.v1".into(),
                    observed_at: "2026-01-01T00:00:00Z".into(),
                    valid_at: None,
                    payload: json!({ "rule": rule }),
                    evidence: Vec::new(),
                    tags: Vec::new(),
                    supersedes: None,
                },
                "writer",
            )
            .expect("append");
        }
        root
    }

    fn hits(root: &Path, query: &str) -> Vec<String> {
        projection::search(
            root,
            &SearchRequest {
                query: Some(query.into()),
                namespace: None,
                type_name: None,
                limit: 10,
            },
        )
        .expect("search")
        .hits
        .into_iter()
        .map(|hit| {
            hit.record.payload["rule"]
                .as_str()
                .expect("rule")
                .to_owned()
        })
        .collect()
    }

    #[test]
    fn one_matching_term_is_enough_and_bm25_still_orders_the_result() {
        let root = store();

        let partial = hits(&root, "deployment nonexistent");
        let spread = hits(&root, "checklist staging grocery");

        assert_eq!(partial, ["Deployment checklist for staging"]);
        assert!(hits(&root, "absent missing").is_empty());
        // Two terms hit the first record and one hits the second: bm25 keeps the
        // denser match on top, so widening candidates never reorders the answer.
        assert_eq!(spread.len(), 2);
        assert_eq!(spread[0], "Deployment checklist for staging");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn a_quoted_term_stays_a_literal_and_never_becomes_an_operator() {
        let root = store();

        // A user-typed quote must not open a phrase, and a bare operator word is
        // matched as text: both queries stay an OR over their literal terms.
        assert_eq!(hits(&root, "\"deployment grocery\"").len(), 2);
        assert_eq!(hits(&root, "deployment AND grocery").len(), 2);
        assert!(hits(&root, "\"").is_empty());
        assert_eq!(fts_query("alpha beta"), "\"alpha\" OR \"beta\"");
        assert_eq!(fts_query("he\"llo"), "\"he\"\"llo\"");
        fs::remove_dir_all(root).expect("cleanup");
    }
}
