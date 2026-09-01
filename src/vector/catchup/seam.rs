//! Putting a test's stand-in back, whatever happens to the test.
//!
//! A seam that clears itself on the line after the body leaves the stand-in
//! installed when the body panics: the assertion unwinds past the line that
//! would have removed it, and the next test on that thread runs against
//! somebody else's substitute. That is worse than a second failure — a leaked
//! pass makes an unreachable provider succeed, so a test that checks what
//! happens when the provider is down goes green having proved the opposite.
//!
//! Restoring the PREVIOUS value rather than clearing to none is what makes a
//! seam inside a seam correct: the inner one hands back what the outer one
//! installed, instead of removing it and leaving the outer body running
//! against production.
//!
//! The same shape the rest of the store already uses to hold temporary state —
//! staged receipts, staged readiness, handoff ownership and both locks all
//! release through `Drop` rather than through a line that a panic can skip.
use std::cell::Cell;
use std::thread::LocalKey;

pub(crate) struct Restore<T: Copy + 'static> {
    slot: &'static LocalKey<Cell<Option<T>>>,
    previous: Option<T>,
}

impl<T: Copy + 'static> Restore<T> {
    /// Install `value`, remembering what was there.
    pub(crate) fn install(slot: &'static LocalKey<Cell<Option<T>>>, value: T) -> Self {
        let previous = slot.with(|cell| cell.replace(Some(value)));
        Self { slot, previous }
    }
}

impl<T: Copy + 'static> Drop for Restore<T> {
    fn drop(&mut self) {
        self.slot.with(|cell| cell.set(self.previous));
    }
}
