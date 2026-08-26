// Provider layout intentionally keeps implementation in provider/<name>/<name>.rs.
#[allow(clippy::module_inception)]
mod sqlite;

mod queries;
mod row;
mod schema;
mod search;
mod writer;

pub use search::search;
pub use sqlite::{initialize, mark_degraded, state, verify};
pub use writer::{index, rebuild};
