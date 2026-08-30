//! Shared scaffolding for the handoff and durability suites: a store with the
//! projection pointed at nothing, and starters that stand in for the detached
//! worker.
use super::{add, configure_unreachable, store};
use crate::kernel::error::Error;
use crate::kernel::lock::TryLock;
use std::cell::Cell;
use std::path::Path;

thread_local! {
    /// Per-thread on purpose: the suite runs in parallel, and a shared counter
    /// would tally starts from whatever else happens to be running.
    static STARTS: Cell<usize> = const { Cell::new(0) };
}

pub(super) fn starts() -> usize {
    STARTS.with(Cell::get)
}

pub(super) fn reset_starts() {
    STARTS.with(|slot| slot.set(0));
}

/// Stands in for spawning the detached worker: the tests are about whether a
/// handoff happens and how often, not about process creation.
pub(super) fn counting_starter(_store: &Path) -> Result<(), Error> {
    STARTS.with(|slot| slot.set(slot.get() + 1));
    Ok(())
}

pub(super) const LOCK: &str = "vector-drain.lock";

/// A configured store that nobody has written to: no target, no ticket, no
/// history. `configured` writes a record for itself, which issues a handoff —
/// fine for handoff tests, fatal for anything asserting their absence.
pub(super) fn bare(name: &str) -> std::path::PathBuf {
    let root = store(name);
    configure_unreachable(&root);
    root
}

/// A configured store with one record already in it.
///
/// Writing that record hands off for itself, which leaves a live claim behind.
/// The fixture clears it, so a test starts from "nobody is working" rather than
/// from the leftovers of its own setup.
pub(super) fn configured(name: &str) -> std::path::PathBuf {
    let root = store(name);
    configure_unreachable(&root);
    add(&root, "the first lesson");
    crate::vector::catchup::handoff::release_for_tests(&root);
    crate::vector::catchup::cooldown::clear(&root);
    root
}

/// A starter that behaves like the real child: it takes the drain lock and
/// keeps it, so the claim handshake sees a genuine takeover.
pub(super) fn taking_starter(store: &Path) -> Result<(), Error> {
    STARTS.with(|slot| slot.set(slot.get() + 1));
    let taken = TryLock::acquire(store, LOCK).expect("lock").expect("free");
    // Keyed by store, and shared rather than thread-local. Thread-local would
    // release the lock when a thread ends, making the next writer look like a
    // winner; a single shared slot would let a parallel test release this one.
    TAKEN
        .lock()
        .expect("holder")
        .insert(store.to_path_buf(), taken);
    Ok(())
}

pub(super) fn release_taken(store: &Path) {
    TAKEN.lock().expect("holder").remove(store);
}

static TAKEN: std::sync::Mutex<std::collections::BTreeMap<std::path::PathBuf, TryLock>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());
