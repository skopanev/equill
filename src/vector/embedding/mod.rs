mod provider;

use super::config::{EmbeddingConfig, VectorConfig};
use super::embedder::Embedder;
use super::model::{self, EmbeddingDescriptor, EmbeddingDocument, bounded_chars};
use crate::kernel::error::Error;
use std::path::Path;

pub use provider::candle::{EMBED_MODEL_ID, MAX_TOKENS, VECTOR_DIMENSIONS};

pub fn instructed_query(instruction: &str, query: &str) -> String {
    format!("Instruct: {instruction}\nQuery:{query}")
}

/// How many characters of the question survive, once the framing a provider
/// adds is counted against the limit.
///
/// The prefix is part of what goes out, so it has to be part of what is
/// counted. Bounding the bare query and letting the provider prepend an
/// instruction afterwards left the outbound string over the limit by however
/// long the instruction was — and the instruction is configurable, so the
/// overrun was configurable too.
///
/// The instruction is kept whole and the question is what gives way: an
/// instruction cut in half tells the model something other than what it said,
/// while a question cut short is still its beginning. A store whose
/// instruction alone fills the limit is a misconfiguration, and it is refused
/// rather than silently reduced to a prefix with nothing asked.
fn room_for_query(framing: Option<&str>, limit: usize) -> Result<usize, Error> {
    let Some(instruction) = framing else {
        return Ok(limit);
    };
    limit
        .checked_sub(instructed_query(instruction, "").chars().count())
        .filter(|room| *room > 0)
        .ok_or_else(|| {
            model::vector_error(
                "query_instruction alone fills max_query_chars; nothing would be asked",
            )
        })
}

pub struct EmbeddingRuntime {
    inner: Runtime,
    max_query_chars: usize,
}

enum Runtime {
    DeepInfra(Box<provider::deepinfra::DeepInfraRuntime>),
    Voyage(Box<provider::voyage::VoyageRuntime>),
    Candle(Box<provider::candle::CandleRuntime>),
    Ollama(Box<provider::ollama::OllamaRuntime>),
}

impl EmbeddingRuntime {
    pub fn load(store: &Path, config: &VectorConfig) -> Result<Self, Error> {
        let inner = match &config.embedding {
            EmbeddingConfig::DeepInfra(embedding) => Runtime::DeepInfra(Box::new(
                provider::deepinfra::DeepInfraRuntime::load(config, embedding)?,
            )),
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
        Ok(Self {
            inner,
            max_query_chars: config.max_query_chars,
        })
    }

    pub fn embed_query(&self, instruction: &str, query: &str) -> Result<Vec<f32>, Error> {
        // Voyage says which kind of text it is being given through `input_type`
        // and adds no prefix, so nothing is reserved for one. The others frame
        // the question in the text itself, and that framing is counted.
        let framing = match &self.inner {
            Runtime::Voyage(_) => None,
            Runtime::DeepInfra(_) | Runtime::Candle(_) | Runtime::Ollama(_) => Some(instruction),
        };
        let query = bounded_chars(query, room_for_query(framing, self.max_query_chars)?);
        match &self.inner {
            Runtime::DeepInfra(value) => value.embed_query(instruction, query),
            Runtime::Voyage(value) => value.embed_query(instruction, query),
            Runtime::Candle(value) => value.embed_query(instruction, query),
            Runtime::Ollama(value) => value.embed_query(instruction, query),
        }
    }
}

impl Embedder for EmbeddingRuntime {
    fn descriptor(&self) -> &EmbeddingDescriptor {
        match &self.inner {
            Runtime::DeepInfra(value) => value.descriptor(),
            Runtime::Voyage(value) => value.descriptor(),
            Runtime::Candle(value) => value.descriptor(),
            Runtime::Ollama(value) => value.descriptor(),
        }
    }

    fn embed(&self, documents: &[EmbeddingDocument]) -> Result<Vec<Vec<f32>>, Error> {
        match &self.inner {
            Runtime::DeepInfra(value) => value.embed(documents),
            Runtime::Voyage(value) => value.embed(documents),
            Runtime::Candle(value) => value.embed(documents),
            Runtime::Ollama(value) => value.embed(documents),
        }
    }
}

#[cfg(test)]
mod tests;
