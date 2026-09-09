use crate::kernel::error::Error;
use std::cell::Cell;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    BeforeAppend,
    AfterAppend,
    BeforeLedgerSync,
    AfterReceipts,
    BeforeOutcome,
    PartialAppend,
    AfterReceipt(usize),
    BeforeProjection,
    AfterBatchDataRemoval,
    AfterAbortReceiptRemoval,
}

thread_local! { static FAILURE: Cell<Option<Step>> = const { Cell::new(None) }; }
thread_local! { static KILL: Cell<bool> = const { Cell::new(false) }; }
type BeforeLock = fn(&Path);
thread_local! { static BEFORE_LOCK: Cell<Option<BeforeLock>> = const { Cell::new(None) }; }

pub(super) fn before_lock_once(action: BeforeLock) {
    BEFORE_LOCK.with(|value| value.set(Some(action)));
}

pub(super) fn before_lock(root: &Path) {
    if let Some(action) = BEFORE_LOCK.with(Cell::take) {
        action(root);
    }
}

pub(super) fn kill_at(step: Step) {
    fail(Some(step));
    KILL.with(|value| value.set(true));
}

pub(super) fn fail(step: Option<Step>) {
    FAILURE.with(|value| value.set(step));
}

pub(super) fn failing(step: Step) -> bool {
    FAILURE.with(|value| value.get()) == Some(step)
}

pub(super) fn at(step: Step) -> Result<(), Error> {
    if FAILURE.with(|value| value.get()) == Some(step) {
        if KILL.with(Cell::get) {
            std::process::abort();
        }
        return Err(Error::Integrity(format!(
            "injected writer interruption at {step:?}"
        )));
    }
    Ok(())
}
