use super::super::model::vector_error;
use crate::kernel::error::Error;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const COOLDOWN: &str = "projections/qdrant/cooldown.json";

/// Doubling, from a second to a minute. Not a retry timer — nothing wakes up
/// when it expires. It only decides whether the NEXT thing that happens to touch
/// this store is allowed to start a worker again.
const FIRST_MS: u128 = 1_000;
const LONGEST_MS: u128 = 60_000;

/// What a store remembers about a catch-up that failed.
///
/// Keyed to the CAUSE of the failure — the endpoint, the model, the config — and
/// deliberately not to the target.
///
/// Keying it to the target was wrong: every new record makes a new target, so a
/// dead provider would be retried once per write, which is the spawn storm this
/// exists to prevent. Writes go on advancing the target regardless; what is held
/// back is starting another worker against a provider that just failed.
///
/// Anything that changes the cause clears the way at once: a fixed endpoint, a
/// new model, a changed config. So does convergence, expiry, and an explicit
/// root sync.
#[derive(Deserialize, Serialize)]
struct Cooldown {
    endpoint: String,
    model_sha256: String,
    config_sha256: String,
    failures: u32,
    eligible_unix_ms: u128,
}

/// Whether starting a worker right now would be repeating a known failure.
pub(super) fn in_effect(store: &Path) -> bool {
    let Some(recorded) = read(store) else {
        return false;
    };
    let Some(situation) = situation(store) else {
        return false;
    };
    if (
        &recorded.endpoint,
        &recorded.model_sha256,
        &recorded.config_sha256,
    ) != (&situation.0, &situation.1, &situation.2)
    {
        // The cause changed: this is not the failure that was recorded.
        return false;
    }
    now_ms() < recorded.eligible_unix_ms
}

/// Remember that this exact attempt failed, and back off further if it keeps
/// failing. Best effort: failing to record a cooldown must not turn a failed
/// catch-up into something worse.
pub(crate) fn record_failure(store: &Path) {
    let Some((endpoint, model_sha256, config_sha256)) = situation(store) else {
        return;
    };
    let failures = read(store)
        .filter(|previous| previous.endpoint == endpoint)
        .map_or(1, |previous| previous.failures.saturating_add(1));
    let wait = FIRST_MS
        .saturating_mul(1_u128 << failures.min(6).saturating_sub(1))
        .min(LONGEST_MS);
    let _ = write(
        store,
        &Cooldown {
            endpoint,
            model_sha256,
            config_sha256,
            failures,
            eligible_unix_ms: now_ms().saturating_add(wait),
        },
    );
}

/// Forget any cooldown. A successful catch-up clears it, and the explicit root
/// sync clears it too — an operator asking for the work is not something a
/// remembered failure gets to refuse.
pub(crate) fn clear(store: &Path) {
    let _ = fs::remove_file(store.join(COOLDOWN));
}

/// The three things that identify a failure cause: where the work was sent and
/// what would have done it. Not what was wanted — that changes with every write.
fn situation(store: &Path) -> Option<(String, String, String)> {
    let config = super::super::config::load(store).ok()??;
    // The config's identity, not its contents. Hashing the file meant reading
    // and digesting it on every write once a cooldown existed — paid on the
    // hottest path to answer a question that "has this file changed?" answers
    // just as well. Size and modification time change together with any edit
    // that could alter the cause of a failure.
    let stamp = fs::metadata(store.join("registry/vector/qdrant.json"))
        .ok()
        .map(|data| {
            let modified = data
                .modified()
                .ok()
                .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|since| since.as_nanos())
                .unwrap_or_default();
            format!("{}:{modified}", data.len())
        })?;
    Some((
        config.endpoint,
        config.embedding.model_sha256().to_owned(),
        stamp,
    ))
}

fn read(store: &Path) -> Option<Cooldown> {
    let bytes = fs::read(store.join(COOLDOWN)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis())
        .unwrap_or_default()
}

fn write(store: &Path, cooldown: &Cooldown) -> Result<(), Error> {
    let path = store.join(COOLDOWN);
    let directory = path
        .parent()
        .ok_or_else(|| vector_error("cooldown directory is invalid"))?;
    fs::create_dir_all(directory)?;
    let temporary = directory.join(format!(".cooldown-{}.json", Uuid::now_v7()));
    let bytes = serde_json::to_vec(cooldown).map_err(|_| vector_error("cooldown serialization"))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    if file
        .write_all(&bytes)
        .and_then(|()| file.sync_all())
        .is_err()
    {
        drop(file);
        let _ = fs::remove_file(&temporary);
        return Err(vector_error("cooldown staging failed"));
    }
    drop(file);
    if fs::rename(&temporary, &path).is_err() {
        let _ = fs::remove_file(&temporary);
        return Err(vector_error("cooldown commit failed"));
    }
    let _ = File::open(directory).and_then(|handle| handle.sync_all());
    Ok(())
}
