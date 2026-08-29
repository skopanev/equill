mod alias;
mod collection;
mod point;
// Provider layout intentionally keeps implementation in provider/<name>/<name>.rs.
#[allow(clippy::module_inception)]
mod qdrant;
mod schema;

pub(crate) use collection::Collection;
pub(crate) use qdrant::{ProviderHit, QdrantTransport, Transport};

#[cfg(test)]
mod runtime_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
mod worker;
