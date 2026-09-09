mod deep;
mod model;
mod policy;
mod provider;
mod scanner;

pub use deep::{DeepReport, audit};
pub use model::{DefenseFinding, DefenseMode, DefenseResult};
pub use policy::{custom_rules, initialize, load};
pub use scanner::apply;

/// Small structured coordinates may be logged only when credential scanning
/// succeeds and finds no secret. A scanner error is a reason to hash the value.
pub(crate) fn coordinate_is_public(value: &str) -> bool {
    provider::secrets_scanner::scan_inline(value).is_ok_and(|scan| scan.matches.is_empty())
}
