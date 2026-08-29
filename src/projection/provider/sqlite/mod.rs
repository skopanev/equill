// Provider layout intentionally keeps implementation in provider/<name>/<name>.rs.
#[allow(clippy::module_inception)]
mod sqlite;

mod queries;
mod row;
mod schema;
mod search;
#[cfg(test)]
mod stemming_tests;
mod writer;

pub use search::{MAX_SCAN, search, superseded};
pub use sqlite::{initialize, mark_degraded, state, verify};
pub use writer::{index, rebuild};
