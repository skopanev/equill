use super::sqlite::failure;
use crate::audit::Event;
use crate::kernel::error::Error;
use rusqlite::{Connection, OptionalExtension, params};
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

pub(super) fn run(root: &Path, connection: &mut Connection) -> Result<(), Error> {
    // Snapshot committed byte boundaries, then release the append lock before
    // any scan or SQL work. A large index rebuild cannot hold writers hostage.
    let lock = crate::audit::writer::lock(root).map_err(failure)?;
    crate::audit::writer::recover(&lock.root).map_err(failure)?;
    let mut months = Vec::new();
    for entry in fs::read_dir(root).map_err(failure)? {
        let path = entry.map_err(failure)?.path();
        if path.extension().is_some_and(|s| s == "jsonl") {
            let month = path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| failure("month"))?;
            let time = format!("{month}-01T00:00:00Z");
            if crate::audit::writer::month(&time)? != month {
                return Err(failure("month"));
            }
            let size = fs::metadata(&path).map_err(failure)?.len();
            months.push((month.to_owned(), path, size));
        }
    }
    months.sort();
    drop(lock);
    let transaction = connection.transaction().map_err(failure)?;
    let known: i64 = transaction
        .query_row("SELECT count(*) FROM sources", [], |row| row.get(0))
        .map_err(failure)?;
    if known > months.len() as i64 {
        return Err(failure("missing ledger"));
    }
    for (month, path, size) in months {
        let mut file = crate::audit::writer::files::open(&path, false).map_err(failure)?;
        let offset: i64 = transaction
            .query_row(
                "SELECT bytes FROM sources WHERE month=?1",
                [&month],
                |row| row.get(0),
            )
            .optional()
            .map_err(failure)?
            .unwrap_or(0);
        let offset = u64::try_from(offset).map_err(failure)?;
        if offset > size {
            return Err(failure("shrunk ledger"));
        }
        file.seek(SeekFrom::Start(offset)).map_err(failure)?;
        let mut reader = BufReader::new(file.take(size - offset));
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line).map_err(failure)? == 0 {
                break;
            }
            if !line.ends_with('\n') || line.len() > 8192 {
                return Err(failure("partial event"));
            }
            let event: Event = serde_json::from_str(&line).map_err(failure)?;
            let at = event
                .observed_at
                .parse::<jiff::Timestamp>()
                .map_err(failure)?;
            if !event.valid() || crate::audit::writer::month(&event.observed_at)? != month {
                return Err(failure("event version or month"));
            }
            transaction
                .execute(
                    "INSERT INTO events VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                    params![
                        event.id.to_string(),
                        at.as_second(),
                        at.subsec_nanosecond(),
                        i64::try_from(event.duration_us).map_err(failure)?,
                        event.surface,
                        event.operation,
                        event.project,
                        event.role,
                        event.process,
                        event.outcome,
                        event.instance,
                        event.session,
                        event.actor_claimed,
                        event.lane_claimed,
                        line.trim_end(),
                    ],
                )
                .map_err(failure)?;
        }
        transaction
            .execute(
                "INSERT OR REPLACE INTO sources VALUES (?1,?2)",
                params![month, i64::try_from(size).map_err(failure)?],
            )
            .map_err(failure)?;
    }
    transaction.commit().map_err(failure)?;
    Ok(())
}
