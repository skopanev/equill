//! Who actually launches the worker: the real spawn in production, and the seam
//! the tests replace so they can assert a handoff without creating processes.
use crate::kernel::error::Error;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

/// How the catch-up gets started. Production always spawns; the tests replace
/// this so they can assert the handoff without launching processes.
///
/// Deliberately NOT an environment variable: a public switch that silently
/// disables the handoff would let a deployment lose automatic indexing without
/// anyone choosing it, and the store would look healthy while falling behind.
pub(crate) type Starter = fn(&Path) -> Result<(), Error>;

#[cfg(test)]
thread_local! {
    static STARTER: std::cell::Cell<Option<Starter>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(crate) fn with_starter<T>(starter: Starter, body: impl FnOnce() -> T) -> T {
    let _restore = super::seam::Restore::install(&STARTER, starter);
    body()
}

pub(super) fn starter() -> Starter {
    #[cfg(test)]
    if let Some(injected) = STARTER.with(|slot| slot.get()) {
        return injected;
    }
    spawn
}

fn spawn(store: &Path) -> Result<(), Error> {
    let executable = std::env::current_exe()?;
    Command::new(executable)
        .arg("vector")
        .arg("drain")
        .arg("--store")
        .arg(store)
        .arg("--once")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // Its own process group, so the child outlives the shell that started
        // the write and never receives that terminal's signals. Without this a
        // Ctrl-C aimed at the command would kill the catch-up too.
        .process_group(0)
        .spawn()?;
    Ok(())
}

/// Bring the text index level, and say whether there is vector work after it.
///
/// Called with the drain lease held, so exactly one process does this at a
/// time. The text index goes first and unconditionally: it is what search answers
/// from, it needs no model and no provider, and a store whose vector projection
/// is off or unreachable must still become searchable.
///
/// A store with no vector projection is then finished. Going on would open a
/// connection the store never asked for and file an error for the absence of
/// something optional.
pub(super) fn catch_text_up_first(store: &std::path::Path) -> bool {
    let _ = crate::projection::catch_up_text(store);
    matches!(super::super::config::load(store), Ok(Some(config)) if config.enabled)
}
