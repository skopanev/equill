// Provider layout intentionally keeps implementation in provider/<name>/<name>.rs.
#[allow(clippy::module_inception)]
mod sqlite;

mod lifecycle;
mod locator;
mod queries;
mod row;
mod schema;
mod search;
#[cfg(test)]
mod stemming_tests;
mod writer;

pub use lifecycle::{historic, history_in_scope};
pub use locator::locators;
pub use search::{MAX_SCAN, search};
pub use sqlite::{clear_degraded, initialize, mark_degraded, state, verify};
pub use writer::{index, index_batch, rebuild};
