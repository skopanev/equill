mod provider;

use super::config::{EmbeddingConfig, VectorConfig};
use super::embedder::Embedder;
use super::model::{EmbeddingDescriptor, EmbeddingDocument};
use crate::kernel::error::Error;
use std::path::Path;

pub use provider::candle::{EMBED_MODEL_ID, MAX_TOKENS, VECTOR_DIMENSIONS};

const MAX_QUERY_CHARS: usize = 2_000;

/// Bound only search input, before any provider-specific instruction prefix.
/// Count Unicode scalar values, not UTF-8 bytes; record documents stay intact.
fn bounded_query(query: &str) -> &str {
    let end = query
        .char_indices()
        .nth(MAX_QUERY_CHARS)
        .map_or(query.len(), |(offset, _)| offset);
    &query[..end]
}

pub fn instructed_query(instruction: &str, query: &str) -> String {
    format!("Instruct: {instruction}\nQuery:{query}")
}

pub struct EmbeddingRuntime {
    inner: Runtime,
}

enum Runtime {
    Voyage(Box<provider::voyage::VoyageRuntime>),
    Candle(Box<provider::candle::CandleRuntime>),
    Ollama(Box<provider::ollama::OllamaRuntime>),
}

impl EmbeddingRuntime {
    pub fn load(store: &Path, config: &VectorConfig) -> Result<Self, Error> {
        let inner = match &config.embedding {
            EmbeddingConfig::Voyage(embedding) => Runtime::Voyage(Box::new(
                provider::voyage::VoyageRuntime::load(config, embedding)?,
            )),
            EmbeddingConfig::Local(embedding) => Runtime::Candle(Box::new(
                provider::candle::CandleRuntime::load(store, config, embedding)?,
            )),
            EmbeddingConfig::Ollama(embedding) => Runtime::Ollama(Box::new(
                provider::ollama::OllamaRuntime::load(config, embedding)?,
            )),
        };
        Ok(Self { inner })
    }

    pub fn embed_query(&self, instruction: &str, query: &str) -> Result<Vec<f32>, Error> {
        let query = bounded_query(query);
        match &self.inner {
            Runtime::Voyage(value) => value.embed_query(instruction, query),
            Runtime::Candle(value) => value.embed_query(instruction, query),
            Runtime::Ollama(value) => value.embed_query(instruction, query),
        }
    }
}

impl Embedder for EmbeddingRuntime {
    fn descriptor(&self) -> &EmbeddingDescriptor {
        match &self.inner {
            Runtime::Voyage(value) => value.descriptor(),
            Runtime::Candle(value) => value.descriptor(),
            Runtime::Ollama(value) => value.descriptor(),
        }
    }

    fn embed(&self, documents: &[EmbeddingDocument]) -> Result<Vec<Vec<f32>>, Error> {
        match &self.inner {
            Runtime::Voyage(value) => value.embed(documents),
            Runtime::Candle(value) => value.embed(documents),
            Runtime::Ollama(value) => value.embed(documents),
        }
    }
}

#[cfg(test)]
mod tests;
