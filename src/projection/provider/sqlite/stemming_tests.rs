use super::sqlite;
use crate::command::init;
use crate::projection::{self, SearchRequest};
use crate::record::{RecordDraft, append_indexed};
use crate::schema::{self, TypeDefinition};
use rusqlite::Connection;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[test]
fn porter_matches_expected_singular_and_derived_terms() {
    let root = store(&["worktree", "fixture", "mutation", "withdraw"]);

    for (query, expected) in [
        ("worktrees", "worktree"),
        ("fixtures", "fixture"),
        ("mutations", "mutation"),
        ("withdrawal", "withdraw"),
    ] {
        assert_eq!(hits(&root, query), [expected]);
    }
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn old_projection_requires_rebuild_and_rebuild_uses_porter() {
    let root = store(&["withdraw"]);
    install_v1_projection(&root);
    let connection = Connection::open(sqlite::database(&root)).expect("old projection");
    let old_hits: i64 = connection
        .query_row(
            "SELECT count(*) FROM records_fts WHERE records_fts MATCH 'withdrawal'",
            [],
            |row| row.get(0),
        )
        .expect("old exact-token search");
    assert_eq!(old_hits, 0);
    drop(connection);

    let error = search(&root, "withdrawal").expect_err("old schema must be explicit");
    assert!(error.to_string().contains("equill rebuild"));
    projection::rebuild(&root).expect("rebuild projection");

    assert_eq!(hits(&root, "withdrawal"), ["withdraw"]);
    let connection = Connection::open(sqlite::database(&root)).expect("rebuilt projection");
    let (version, create_sql): (String, String) = connection
        .query_row(
            "SELECT (SELECT value FROM equill_meta WHERE key='schema_version'), sql
             FROM sqlite_master WHERE name='records_fts'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("rebuilt schema");
    // Compared against the schema the build declares rather than a literal: a
    // version bump is how an old projection gets refused, so pinning the number
    // here would turn every bump into a failing test about nothing.
    assert_eq!(version, super::schema::VERSION);
    assert!(create_sql.contains("porter unicode61 remove_diacritics 2"));
    fs::remove_dir_all(root).expect("cleanup");
}

fn store(rules: &[&str]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("equill-stem-{}", Uuid::now_v7()));
    init::create(&root, "writer", "agent.memory").expect("initialize");
    schema::register(
        &root,
        TypeDefinition {
            type_name: "agent.lesson.v1".into(),
            uri: "equill://agent.lesson/v1".into(),
            owner: "writer".into(),
            payload_schema: json!({
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
    for rule in rules {
        append_indexed(
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

fn search(
    root: &Path,
    query: &str,
) -> Result<projection::SearchReport, crate::kernel::error::Error> {
    projection::search(
        root,
        &SearchRequest {
            query: Some(query.into()),
            namespace: None,
            type_name: None,
            limit: 10,
        },
    )
}

fn hits(root: &Path, query: &str) -> Vec<String> {
    search(root, query)
        .expect("search")
        .hits
        .into_iter()
        .map(|hit| hit.record.payload["rule"].as_str().expect("rule").into())
        .collect()
}

fn install_v1_projection(root: &Path) {
    let connection = Connection::open(sqlite::database(root)).expect("projection");
    connection
        .execute_batch(
            "DROP TABLE records_fts;
             CREATE VIRTUAL TABLE records_fts USING fts5(
               id UNINDEXED, content, tokenize='unicode61 remove_diacritics 2'
             );
             INSERT INTO records_fts(id, content)
               SELECT id, payload_json || ' ' || evidence_json || ' ' || tags_json FROM records;
             UPDATE equill_meta SET value='1' WHERE key='schema_version';",
        )
        .expect("install old projection");
}
