//! The seams that let the bound be asserted without paying it, and the real
//! values production always uses.
//!
//! Both are deliberately compiled out of a release build rather than exposed as
//! settings. A knob that shortened the bound would let a deployment turn a
//! worker that finishes into one that gives up early, and the store would look
//! like it was catching up while never converging; a knob that replaced the
//! pass would let it stop doing the work entirely. The tests need a bound they
//! can reach in milliseconds and a pass that never converges — production needs
//! neither, and gets neither.
use super::super::operator;
use crate::kernel::error::Error;
use std::path::Path;
use std::time::Duration;

/// Bounds that keep a worker finite no matter what the ledger or the provider
/// does. Neither is a tuning knob for throughput — they exist so that a process
/// which cannot make progress cannot keep running either.
const MAX_PASSES: usize = 64;
const DEADLINE: Duration = Duration::from_secs(900);

#[cfg(test)]
thread_local! {
    // How many passes and how long, for the length of one test.
    static BOUNDS: std::cell::Cell<Option<(usize, Duration)>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(crate) fn with_bounds<T>(passes: usize, deadline: Duration, body: impl FnOnce() -> T) -> T {
    BOUNDS.with(|slot| slot.set(Some((passes, deadline))));
    let outcome = body();
    BOUNDS.with(|slot| slot.set(None));
    outcome
}

pub(super) fn bounds() -> (usize, Duration) {
    #[cfg(test)]
    if let Some(injected) = BOUNDS.with(std::cell::Cell::get) {
        return injected;
    }
    (MAX_PASSES, DEADLINE)
}

/// One pass of the catch-up.
///
/// A pass against a real provider is the only way the loop ever gets as far as
/// its bound: an unreachable one fails on the first request, so the worker
/// stops for that reason instead and the bound is never reached. Standing in
/// for the pass is what lets the bound be asserted at all — the same trick
/// `Starter` plays for the fork.
pub(crate) type Pass = fn(&Path) -> Result<operator::VectorSyncReport, Error>;

#[cfg(test)]
thread_local! {
    // The stand-in pass, for the length of one test.
    static PASS: std::cell::Cell<Option<Pass>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(crate) fn with_pass<T>(pass: Pass, body: impl FnOnce() -> T) -> T {
    PASS.with(|slot| slot.set(Some(pass)));
    let outcome = body();
    PASS.with(|slot| slot.set(None));
    outcome
}

pub(super) fn pass() -> Pass {
    #[cfg(test)]
    if let Some(injected) = PASS.with(std::cell::Cell::get) {
        return injected;
    }
    operator::catch_up
}
