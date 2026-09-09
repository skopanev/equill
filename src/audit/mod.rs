//! Isolated request history. Immutable monthly ledgers are truth; the local
//! selection index is disposable. No project store or vector service is used.
mod boundary;
mod capture;
mod model;
mod projection;
mod query;
pub(crate) mod result;
mod writer;

pub(crate) use boundary::error_class;
pub use boundary::{Invocation, destination};
pub use model::{Arguments, Event, Output};
pub use query::{Listing, Scope, Statistics, list, list_at, stats, stats_at};

#[cfg(test)]
mod tests;
