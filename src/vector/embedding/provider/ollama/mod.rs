// Provider layout intentionally keeps implementation in provider/<name>/<name>.rs.
#[allow(clippy::module_inception)]
mod ollama;

pub(in crate::vector::embedding) use ollama::OllamaRuntime;

#[cfg(test)]
mod tests;
