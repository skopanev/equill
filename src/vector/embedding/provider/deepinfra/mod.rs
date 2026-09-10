// Provider layout intentionally keeps implementation in provider/<name>/<name>.rs.
#[allow(clippy::module_inception)]
mod deepinfra;

mod key;
mod transport;

pub(in crate::vector::embedding) use deepinfra::DeepInfraRuntime;

#[cfg(test)]
mod tests;
