// Provider layout intentionally keeps implementation in provider/<name>/<name>.rs.
#[allow(clippy::module_inception)]
mod manifest;

pub use manifest::ManifestResolver;
