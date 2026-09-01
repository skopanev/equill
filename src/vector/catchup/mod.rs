//! Keeping the index level with the ledger: what is wanted, who is allowed to
//! chase it, who is doing so now, and how that survives a crash.
pub(crate) mod bounds;
pub(crate) mod cooldown;
pub(crate) mod desired;
pub(crate) mod drain;
pub(crate) mod handoff;
/// Test-only: the seams that use it are themselves compiled out of a release
/// build, so the guard has nobody to serve there either.
#[cfg(test)]
pub(crate) mod seam;
pub(crate) mod starter;
pub(crate) mod worker;
