//! What the confirmation boundary touched, counted.
//!
//! The end-to-end benchmark shows that confirmation does not get slower as a
//! store grows, which is the outcome that matters — but on a shared machine a
//! slope can be muddied by load, and absence is hard to prove by timing. These
//! counters prove it directly: a test drives one canonical append and then
//! asserts that nothing scanned the ledger, rebuilt the lifecycle graph, or
//! opened a projection transaction before the caller was told the record is
//! durable.
//!
//! Test-only in the strictest sense — the whole module compiles away outside
//! `cfg(test)`, so production carries neither the counters nor the branches
//! that would maintain them.
#![cfg(test)]

use std::cell::Cell;

thread_local! {
    static LEDGER_READS: Cell<usize> = const { Cell::new(0) };
    static LIFECYCLE_WALKS: Cell<usize> = const { Cell::new(0) };
    static PROJECTION_WRITES: Cell<usize> = const { Cell::new(0) };
}

/// A reading of what happened since the last reset.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Touched {
    pub(crate) ledger_reads: usize,
    pub(crate) lifecycle_walks: usize,
    pub(crate) projection_writes: usize,
}

pub(crate) fn ledger_read() {
    LEDGER_READS.with(|count| count.set(count.get() + 1));
}

pub(crate) fn lifecycle_walk() {
    LIFECYCLE_WALKS.with(|count| count.set(count.get() + 1));
}

pub(crate) fn projection_write() {
    PROJECTION_WRITES.with(|count| count.set(count.get() + 1));
}

pub(crate) fn reset() {
    LEDGER_READS.with(|count| count.set(0));
    LIFECYCLE_WALKS.with(|count| count.set(0));
    PROJECTION_WRITES.with(|count| count.set(0));
}

pub(crate) fn touched() -> Touched {
    Touched {
        ledger_reads: LEDGER_READS.with(Cell::get),
        lifecycle_walks: LIFECYCLE_WALKS.with(Cell::get),
        projection_writes: PROJECTION_WRITES.with(Cell::get),
    }
}
