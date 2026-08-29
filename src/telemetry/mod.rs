#[cfg(test)]
mod tests;

use crate::kernel::error::Error;
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

const LOG: &str = "diagnostics/queries.jsonl";
const ENABLE_ENV: &str = "EQUILL_QUERY_LOG";

/// One answered question. The interesting rows are the ones with no results: a
/// query that found nothing while a matching record existed is the number that
/// decides whether better retrieval is worth building, and counting it by hand
/// means remembering to count, which nobody does.
///
/// The query text is recorded because it is also the input for any stemming or
/// synonym work — the words people actually type. Record payloads never are.
#[derive(Debug, Serialize)]
struct QueryEntry<'a> {
    at: String,
    surface: &'a str,
    query: &'a str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    coordinates: Vec<&'a str>,
    results: usize,
    miss: bool,
}

/// Appending must never be able to fail a query that already succeeded, so the
/// caller gets no error to handle: diagnostics are worth less than the answer.
pub fn record_query(
    store_root: &Path,
    surface: &str,
    query: &str,
    coordinates: Vec<&str>,
    results: usize,
) {
    if !enabled() {
        return;
    }
    let _ = write(store_root, surface, query, coordinates, results);
}

/// Off unless the store's operator turns it on. Nothing here leaves the machine
/// — the log is a file inside the store the caller already owns — but a query is
/// still the caller's own words, and writing those down is the operator's
/// decision rather than one this executable makes for them.
fn enabled() -> bool {
    std::env::var(ENABLE_ENV).is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
}

fn write(
    store_root: &Path,
    surface: &str,
    query: &str,
    coordinates: Vec<&str>,
    results: usize,
) -> Result<(), Error> {
    let entry = QueryEntry {
        at: jiff::Timestamp::now().to_string(),
        surface,
        query,
        coordinates,
        results,
        miss: results == 0,
    };
    let path = store_root.join(LOG);
    if let Some(directory) = path.parent() {
        fs::create_dir_all(directory)?;
    }
    let mut line = serde_json::to_vec(&entry)?;
    line.push(b'\n');
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(&line)?;
    Ok(())
}

/// Reads the log back so the miss rate can be answered without a shell pipeline.
pub fn misses(store_root: &Path) -> Result<(usize, usize), Error> {
    let path = store_root.join(LOG);
    if !path.is_file() {
        return Ok((0, 0));
    }
    let contents = fs::read_to_string(path)?;
    let mut total = 0;
    let mut missed = 0;
    for line in contents.lines().filter(|line| !line.trim().is_empty()) {
        let entry: serde_json::Value = serde_json::from_str(line)?;
        total += 1;
        if entry.get("miss").and_then(serde_json::Value::as_bool) == Some(true) {
            missed += 1;
        }
    }
    Ok((total, missed))
}
