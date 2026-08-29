pub const VERSION: &str = "2";

pub const CREATE: &str = r#"
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS equill_meta (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
) STRICT;
INSERT OR IGNORE INTO equill_meta(key, value) VALUES ('schema_version', '2');

CREATE TABLE IF NOT EXISTS records (
  id TEXT PRIMARY KEY,
  namespace TEXT NOT NULL,
  type_name TEXT NOT NULL,
  actor TEXT NOT NULL,
  recorded_at TEXT NOT NULL,
  observed_at TEXT NOT NULL,
  valid_at TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  evidence_json TEXT NOT NULL,
  tags_json TEXT NOT NULL,
  supersedes TEXT,
  record_sha256 TEXT NOT NULL,
  ledger TEXT NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS records_namespace_type
  ON records(namespace, type_name, valid_at);
CREATE INDEX IF NOT EXISTS records_recorded_at ON records(recorded_at);

CREATE VIRTUAL TABLE IF NOT EXISTS records_fts USING fts5(
  id UNINDEXED,
  content,
  tokenize = 'porter unicode61 remove_diacritics 2'
);
"#;
