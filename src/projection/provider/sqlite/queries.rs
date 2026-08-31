pub const INSERT_RECORD: &str = r#"
INSERT OR IGNORE INTO records(
  id, namespace, type_name, actor, recorded_at, observed_at, valid_at,
  payload_json, evidence_json, tags_json, supersedes, revoked,
  record_sha256, ledger
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
"#;

/// A record a later one replaced. Written when the replacement is indexed,
/// which is the moment the fact exists.
pub const MARK_SUPERSEDED: &str = r#"
UPDATE records SET superseded = 1 WHERE id = ?1
"#;

/// The same fact, discovered from the other side. A rebuild indexes whatever
/// order the ledger holds, so a record may arrive after the one that replaced
/// it; without this it would keep answering as current.
pub const MARK_IF_ALREADY_REPLACED: &str = r#"
UPDATE records SET superseded = 1
WHERE id = ?1 AND EXISTS (SELECT 1 FROM records WHERE supersedes = ?1)
"#;

/// History is excluded before the page is cut, not after: filtering a limited
/// result set would return nothing when the top hit happens to be a record a
/// later one replaced, while a live match waited one row below.
pub const SEARCH: &str = r#"
SELECT r.id, r.namespace, r.type_name, r.actor, r.recorded_at, r.observed_at,
       r.valid_at, r.payload_json, r.evidence_json, r.tags_json, r.supersedes
FROM records_fts f
JOIN records r ON r.id = f.id
WHERE records_fts MATCH ?1
  AND (?2 IS NULL OR r.namespace = ?2)
  AND (?3 IS NULL OR r.type_name = ?3)
  AND r.superseded = 0
  AND r.revoked = 0
ORDER BY bm25(records_fts), r.recorded_at DESC, r.id
LIMIT ?4
"#;

/// Selecting by filter alone still goes through the projection: the caller has
/// no text to match, but reading the whole ledger to answer would step outside
/// the boundary and past the scope they asked for. Ordered by id so the same
/// question returns the same page.
pub const SCAN: &str = r#"
SELECT id, namespace, type_name, actor, recorded_at, observed_at, valid_at,
       payload_json, evidence_json, tags_json, supersedes
FROM records
WHERE (?1 IS NULL OR namespace = ?1)
  AND (?2 IS NULL OR type_name = ?2)
  AND superseded = 0
  AND revoked = 0
ORDER BY id
LIMIT ?3
"#;

/// How much of a scope is history: replaced by a later record, or withdrawn.
pub const HISTORY_IN_SCOPE: &str = r#"
SELECT count(*) FROM records
WHERE (?1 IS NULL OR namespace = ?1)
  AND (?2 IS NULL OR type_name = ?2)
  AND (superseded = 1 OR revoked = 1)
"#;

pub const RECORD_BY_ID: &str = r#"
SELECT id, namespace, type_name, actor, recorded_at, observed_at, valid_at,
       payload_json, evidence_json, tags_json, supersedes
FROM records
WHERE id = ?1
"#;
