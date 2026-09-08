//! Compaction for a store that was written record by record.
mod apply;
mod plan;
#[cfg(test)]
mod projection_tests;
mod projections;
#[cfg(test)]
mod race_tests;
mod run;

pub use apply::{drop_receipts, rewrite, stage_records};
pub use plan::{Plan, Removal, Removed, Severed, build};
pub use run::{NativeReport, run};

#[cfg(test)]
mod plan_tests;
