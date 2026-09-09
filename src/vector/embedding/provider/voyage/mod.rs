// Provider layout intentionally keeps implementation in provider/<name>/<name>.rs.
#[allow(clippy::module_inception)]
mod voyage;

mod transport;

pub(in crate::vector::embedding) use voyage::VoyageRuntime;

#[cfg(test)]
mod tests;
