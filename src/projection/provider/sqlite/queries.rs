pub const INSERT_RECORD: &str = r#"
INSERT OR IGNORE INTO records(
  id, namespace, type_name, actor, recorded_at, observed_at, valid_at,
  payload_json, evidence_json, tags_json, supersedes, record_sha256, ledger
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
"#;

pub const SEARCH: &str = r#"
SELECT r.id, r.namespace, r.type_name, r.actor, r.recorded_at, r.observed_at,
       r.valid_at, r.payload_json, r.evidence_json, r.tags_json, r.supersedes
FROM records_fts f
JOIN records r ON r.id = f.id
WHERE records_fts MATCH ?1
  AND (?2 IS NULL OR r.namespace = ?2)
  AND (?3 IS NULL OR r.type_name = ?3)
ORDER BY bm25(records_fts), r.recorded_at DESC, r.id
LIMIT ?4
"#;

pub const RECORD_BY_ID: &str = r#"
SELECT id, namespace, type_name, actor, recorded_at, observed_at, valid_at,
       payload_json, evidence_json, tags_json, supersedes
FROM records
WHERE id = ?1
"#;
