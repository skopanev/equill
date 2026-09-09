//! Compaction for a store that was written record by record.
mod apply;
#[cfg(test)]
mod crash_tests;
mod journal;
mod projections;
mod recover;
mod run;
#[cfg(test)]
mod settle_tests;

pub use apply::{drop_receipts, rewrite, stage_records};
pub use projections::{Plan, Removal, Removed, Severed, build};
pub use run::{NativeReport, run};

#[cfg(test)]
mod plan_tests;
#[cfg(test)]
mod vector_tests;
