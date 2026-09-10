//! How much text an embedding input may carry.
use crate::kernel::error::Error;
use crate::vector::model::vector_error;

/// The default both limits take when a descriptor does not name them.
pub const DEFAULT_MAX_CHARS: usize = 2_000;

/// Not a provider limit — a sanity bound. Past this the number is a mistake,
/// and a mistake that large is cheaper to refuse than to embed.
const MAX_CHARS_CEILING: usize = 1_000_000;

pub(super) fn default_chars() -> usize {
    DEFAULT_MAX_CHARS
}

/// Zero is refused rather than read as "no limit": a store that means unbounded
/// has to say a number, because a silent zero would embed nothing and look like
/// a configuration that worked.
pub(super) fn validate(limit: usize, name: &str) -> Result<(), Error> {
    if !(1..=MAX_CHARS_CEILING).contains(&limit) {
        return Err(vector_error(&format!(
            "{name} must be between 1 and {MAX_CHARS_CEILING}"
        )));
    }
    Ok(())
}
