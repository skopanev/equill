//! Counters proving the skipped query path was not walked.
//!
//! A test can watch what came back and conclude the search found nothing, but
//! finding nothing and never being asked look identical from the outside — and
//! the promise the setting makes is about the calls, not the results. These
//! count entries into the two halves of the query path so the difference is
//! measured rather than argued.
use std::cell::Cell;

thread_local! {
    static FTS: Cell<usize> = const { Cell::new(0) };
    static SEMANTIC: Cell<usize> = const { Cell::new(0) };
}

pub(crate) fn entered_fts() {
    FTS.with(|count| count.set(count.get() + 1));
}

pub(crate) fn entered_semantic() {
    SEMANTIC.with(|count| count.set(count.get() + 1));
}

/// Both counts, cleared, so a test states its own starting point.
pub(crate) fn reset() {
    FTS.with(|count| count.set(0));
    SEMANTIC.with(|count| count.set(0));
}

/// Entries into the text half and the semantic half, in that order.
pub(crate) fn counts() -> (usize, usize) {
    (FTS.with(Cell::get), SEMANTIC.with(Cell::get))
}
