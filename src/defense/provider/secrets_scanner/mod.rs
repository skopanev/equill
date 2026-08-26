// Provider layout intentionally keeps implementation in provider/<name>/<name>.rs.
#[allow(clippy::module_inception)]
mod secrets_scanner;

pub use secrets_scanner::{Match, scan_custom, scan_deep, scan_inline};
