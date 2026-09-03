mod provider;

use super::config::{EmbeddingConfig, VectorConfig};
use super::embedder::Embedder;
use super::model::{EmbeddingDescriptor, EmbeddingDocument};
use crate::kernel::error::Error;
use std::path::Path;

pub use provider::candle::{EMBED_MODEL_ID, MAX_TOKENS, VECTOR_DIMENSIONS};

pub const QUERY_PREFIX: &str =
    "Instruct: Given a web search query, retrieve relevant passages that answer the query\nQuery:";

pub struct EmbeddingRuntime {
    inner: Runtime,
}

enum Runtime {
    Candle(Box<provider::candle::CandleRuntime>),
    Ollama(Box<provider::ollama::OllamaRuntime>),
}

impl EmbeddingRuntime {
    pub fn load(store: &Path, config: &VectorConfig) -> Result<Self, Error> {
        let inner = match &config.embedding {
            EmbeddingConfig::Local(embedding) => Runtime::Candle(Box::new(
                provider::candle::CandleRuntime::load(store, config, embedding)?,
            )),
            EmbeddingConfig::Ollama(embedding) => Runtime::Ollama(Box::new(
                provider::ollama::OllamaRuntime::load(config, embedding)?,
            )),
        };
        Ok(Self { inner })
    }

    pub fn embed_query(&self, query: &str) -> Result<Vec<f32>, Error> {
        match &self.inner {
            Runtime::Candle(value) => value.embed_query(query),
            Runtime::Ollama(value) => value.embed_query(query),
        }
    }
}

impl Embedder for EmbeddingRuntime {
    fn descriptor(&self) -> &EmbeddingDescriptor {
        match &self.inner {
            Runtime::Candle(value) => value.descriptor(),
            Runtime::Ollama(value) => value.descriptor(),
        }
    }

    fn embed(&self, documents: &[EmbeddingDocument]) -> Result<Vec<Vec<f32>>, Error> {
        match &self.inner {
            Runtime::Candle(value) => value.embed(documents),
            Runtime::Ollama(value) => value.embed(documents),
        }
    }
}

#[cfg(test)]
mod tests;
