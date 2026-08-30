use super::super::model::vector_error;
use crate::kernel::error::Error;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const TICKET: &str = "projections/qdrant/handoff.json";
/// Where a consumed claim lives while its worker runs. Consuming has to hand
/// ownership over, not drop it: deleting the claim outright left a window where
/// the worker was running but nothing said so, and the next writer started a
/// second one.
const ACTIVE: &str = "projections/qdrant/handoff-active.json";

/// How long an unconsumed claim is believed. A child that was started and then
/// died before taking the drain lock leaves one behind; past this it is treated
/// as stale so a store cannot be wedged by one failed spawn. Long enough that a
/// slow machine starting a process is never mistaken for a dead one.
const STALE_AFTER_MS: u128 = 30_000;

/// A single-use permission to run one catch-up, and the thing that stops two
/// writers from starting two workers.
///
/// Creating it is atomic — `create_new` fails if one exists — so the claim
/// itself is the mutual exclusion. The parent does not wait to see whether the
/// child took over; it either created the claim, in which case it is the one
/// starting a worker, or it did not, in which case somebody else already is.
/// That is what keeps the append path free of any wait.
///
/// It is deliberately not a secret. It cannot stop somebody who can already
/// write to the store directory; it stops the worker entry point from being a
/// standing, unauthenticated route to work that `vector sync` gates on root.
#[derive(Deserialize, Serialize)]
struct Claim {
    id: Uuid,
    issued_unix_ms: u128,
}

/// Try to become the one who starts a worker.
///
/// `Ok(Some(id))` means this caller created the claim and should start a child.
/// `Ok(None)` means a live claim already exists: somebody else is starting or
/// running a worker, and that worker will see this revision because it re-reads
/// the target on every pass.
pub(crate) fn claim(store: &Path) -> Result<Option<Uuid>, Error> {
    let path = store.join(TICKET);
    let directory = path
        .parent()
        .ok_or_else(|| vector_error("handoff directory is invalid"))?;
    fs::create_dir_all(directory)?;
    // A claim that has not been taken up yet: somebody is starting a worker.
    match read(&path)? {
        Some(existing) if !stale(&existing) => return Ok(None),
        // Stale: the child it was issued for is long gone. Clear it so the
        // exclusive create below can win.
        Some(_) => {
            let _ = fs::remove_file(&path);
        }
        None => {}
    }
    // A claim that HAS been taken up: a worker is running — unless it died. The
    // operating system releases the drain lock when a process ends, so a free
    // lock beside an ownership marker is a worker that is gone. Waiting for the
    // marker to age out instead would leave a store idle for the whole stale
    // window after any kill.
    if read(&store.join(ACTIVE))?.is_some_and(|owner| !stale(&owner)) && working(store) {
        return Ok(None);
    }
    // Create the claim file ITSELF exclusively. Staging to a temp and renaming
    // would not do: rename replaces, so two callers could both "create" one and
    // both start a worker. create_new on the real path is the exclusion.
    let id = Uuid::now_v7();
    let claim = Claim {
        id,
        issued_unix_ms: now_ms(),
    };
    match create_exclusively(&path, &claim) {
        Ok(()) => {}
        // Somebody created it between the read above and here.
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => return Ok(None),
        Err(_) => return Err(vector_error("handoff staging failed")),
    }
    let _ = File::open(directory).and_then(|handle| handle.sync_all());
    Ok(Some(id))
}

/// Take the claim, or refuse.
///
/// The rename is the atomic step: two workers racing here cannot both succeed,
/// because only one rename-away survives. A worker consumes the claim as it
/// takes ownership, so a claim never outlives the work it authorized.
pub(crate) fn consume(store: &Path) -> Result<Ownership, Error> {
    let path = store.join(TICKET);
    let active = store.join(ACTIVE);
    // One rename, so two workers racing here cannot both win — and the claim
    // becomes the ownership marker rather than disappearing, so a writer
    // arriving mid-run still sees that somebody is on it.
    if fs::rename(&path, &active).is_err() {
        return Err(vector_error(
            "no handoff to consume: this worker is started by a write, not by hand",
        ));
    }
    Ok(Ownership { path: active })
}

/// Held for as long as a worker is working. Dropping it says the work is over,
/// which is what lets the next writer start a fresh one.
pub(crate) struct Ownership {
    path: std::path::PathBuf,
}

impl Drop for Ownership {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Give the claim back without doing the work. Used when a start fails, so one
/// refused spawn does not stop the next writer from trying.
pub(crate) fn release(store: &Path) {
    let _ = fs::remove_file(store.join(TICKET));
    let _ = fs::remove_file(store.join(ACTIVE));
}

fn read(path: &Path) -> Result<Option<Claim>, Error> {
    if !path.is_file() {
        return Ok(None);
    }
    // An unreadable claim is treated as absent rather than fatal: it is a hint
    // about who is working, not a record anything depends on.
    Ok(fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok()))
}

/// Whether somebody is holding the drain lock right now. Acquiring it and
/// letting it go immediately is the cheapest way to ask.
fn working(store: &Path) -> bool {
    match crate::kernel::lock::TryLock::acquire(store, super::worker::LOCK) {
        Ok(Some(probe)) => {
            drop(probe);
            false
        }
        // Held: a live worker. Unusable: assume somebody is there rather than
        // start a competing one.
        _ => true,
    }
}

fn stale(claim: &Claim) -> bool {
    now_ms().saturating_sub(claim.issued_unix_ms) > STALE_AFTER_MS
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis())
        .unwrap_or_default()
}

/// Create the file or fail because it already exists. The failure IS the answer:
/// exactly one caller can win, and it wins without any lock or wait.
fn create_exclusively(path: &Path, claim: &Claim) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(claim).unwrap_or_default();
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(&bytes)?;
    file.sync_all()
}

#[cfg(test)]
pub(crate) fn release_for_tests(store: &Path) {
    release(store);
}

#[cfg(test)]
pub(crate) fn path(store: &Path) -> std::path::PathBuf {
    store.join(TICKET)
}
