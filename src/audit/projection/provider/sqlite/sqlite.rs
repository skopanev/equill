use crate::audit::writer::files;
use crate::kernel::error::Error;
use fs2::FileExt;
use rusqlite::Connection;
use std::fs::File;
use std::path::Path;

const SCHEMA: &str = "
PRAGMA user_version=2;
CREATE TABLE IF NOT EXISTS sources (month TEXT PRIMARY KEY, bytes INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS events (
 id TEXT PRIMARY KEY, at INTEGER NOT NULL, nanos INTEGER NOT NULL, duration INTEGER NOT NULL,
 surface TEXT NOT NULL, operation TEXT NOT NULL, project TEXT, role TEXT,
 process TEXT, outcome TEXT NOT NULL, instance TEXT, session TEXT, actor TEXT,
 lane TEXT, event TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS audit_time ON events(at, nanos, id);
CREATE INDEX IF NOT EXISTS audit_surface ON events(surface, at);
CREATE INDEX IF NOT EXISTS audit_operation ON events(operation, at);
CREATE INDEX IF NOT EXISTS audit_project ON events(project, at);
CREATE INDEX IF NOT EXISTS audit_role ON events(role, at);
CREATE INDEX IF NOT EXISTS audit_process ON events(process, at);
CREATE INDEX IF NOT EXISTS audit_outcome ON events(outcome, at);
CREATE INDEX IF NOT EXISTS audit_instance ON events(instance, at);
CREATE INDEX IF NOT EXISTS audit_session ON events(session, at);
CREATE INDEX IF NOT EXISTS audit_actor ON events(actor, at);
CREATE INDEX IF NOT EXISTS audit_lane ON events(lane, at);
";

pub(super) fn failure(_: impl std::fmt::Display) -> Error {
    Error::Audit("audit projection or immutable ledger is inconsistent".into())
}

pub(super) fn ready(root: &Path) -> Result<(Connection, File), Error> {
    // SQLite's NOFOLLOW also rejects symlinked directory components (including
    // common system temporary-directory aliases). The already validated audit
    // root is resolved once; child paths still remain no-follow.
    let canonical = std::fs::canonicalize(root).map_err(failure)?;
    let root = canonical.as_path();
    let index_lock = files::open(&root.join("index.lock"), true).map_err(failure)?;
    index_lock.lock_exclusive().map_err(failure)?;
    let path = root.join("index.sqlite3");
    let mut connection = open(&path)?;
    let valid = connection
        .query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0))
        .is_ok_and(|v| v == "ok")
        && connection
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .is_ok_and(|v| v == 2);
    if !valid {
        drop(connection);
        // This file contains a rebuildable projection, never canonical events.
        files::remove(&path).map_err(failure)?;
        connection = open(&path)?;
    }
    if connection
        .execute_batch(SCHEMA)
        .map_err(failure)
        .and_then(|_| super::catchup::run(root, &mut connection))
        .is_err()
    {
        // A damaged or stale index gets one rebuild. Invalid source truth must
        // still fail, rather than being skipped to make the query succeed.
        drop(connection);
        files::remove(&path).map_err(failure)?;
        connection = open(&path)?;
        connection.execute_batch(SCHEMA).map_err(failure)?;
        super::catchup::run(root, &mut connection)?;
    }
    Ok((connection, index_lock))
}

fn open(path: &Path) -> Result<Connection, Error> {
    drop(files::open(path, true).map_err(failure)?);
    for suffix in ["-journal", "-wal", "-shm"] {
        let child = path.with_file_name(format!("index.sqlite3{suffix}"));
        if std::fs::symlink_metadata(&child).is_ok() {
            drop(files::open(&child, false).map_err(failure)?);
        }
    }
    Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::default() | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(failure)
}
