mod catalog;
mod lookup;
mod model;
mod registry;
mod validation;

#[cfg(test)]
mod tests;

pub use catalog::{CatalogReport, TypeReport, list, show};
pub use lookup::{load, verify_all};
pub use model::{LifecycleMode, LifecyclePolicy, TypeDefinition};
pub use registry::{RegisterReport, register, register_file};
