//! Keeping the index level with the ledger: what is wanted, who is allowed to
//! chase it, who is doing so now, and how that survives a crash.
pub(crate) mod bounds;
pub(crate) mod cooldown;
pub(crate) mod desired;
pub(crate) mod drain;
pub(crate) mod handoff;
pub(crate) mod starter;
pub(crate) mod worker;
