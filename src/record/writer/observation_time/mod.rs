//! Fresh observation guard; existing ledger validation deliberately does not use it.
use crate::kernel::error::Error;
use jiff::Timestamp;

const MAX_FUTURE_SKEW_SECONDS: i128 = 60;

pub(super) fn validate(observed_at: &str, recorded_at: &str) -> Result<(), Error> {
    let parse = |value: &str, field| {
        value
            .parse::<Timestamp>()
            .map_err(|_| Error::InvalidRecord(format!("{field} must be an RFC3339 timestamp")))
    };
    let observed = parse(observed_at, "observed_at")?;
    let recorded = parse(recorded_at, "recorded_at")?;
    // Nanoseconds in i128 cover the full Timestamp range without adding to MAX.
    let lead = observed
        .as_nanosecond()
        .checked_sub(recorded.as_nanosecond())
        .ok_or_else(|| Error::InvalidRecord("observation time difference overflow".into()))?;
    if lead > MAX_FUTURE_SKEW_SECONDS * 1_000_000_000 {
        return Err(Error::InvalidRecord(format!(
            "observed_at exceeds writer recorded_at by more than {MAX_FUTURE_SKEW_SECONDS} seconds"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod arithmetic_tests;
#[cfg(test)]
mod historical_fixture;
#[cfg(test)]
mod import_tests;
#[cfg(test)]
mod preservation_tests;
#[cfg(test)]
mod write_tests;
