use super::config::VectorConfig;
use super::embedder::Embedder;
use super::model::{EmbeddingDescriptor, EmbeddingDocument, vector_error};
use crate::kernel::error::Error;
use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::qwen3::{Config, Model};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokenizers::{Tokenizer, TruncationParams};

/// The embedding contract, read from the model's own artifacts rather than
/// assumed: `1_Pooling/config.json` sets `pooling_mode_lasttoken`,
/// `modules.json` ends with Normalize, `sentence_bert_config.json` caps the
/// sequence at 512, `config.json` declares 1024 hidden units over 28 layers,
/// and `config_sentence_transformers.json` gives the retrieval instruction for
/// queries and an empty prompt for documents.
///
/// This is a decoder model, so pooling takes the last token rather than a CLS
/// position, and the query instruction is a full instruct preamble rather than
/// a short prefix. Changing any of this changes what a stored vector means.
pub const EMBED_MODEL_ID: &str = "Qwen/Qwen3-Embedding-0.6B";
pub const VECTOR_DIMENSIONS: u64 = 1024;
pub const MAX_TOKENS: usize = 512;
pub const QUERY_PREFIX: &str =
    "Instruct: Given a web search query, retrieve relevant passages that answer the query\nQuery:";

pub struct EmbeddingRuntime {
    descriptor: EmbeddingDescriptor,
    /// Never used directly. Every embedding runs on a clone, because a decoder
    /// keeps a key/value cache across calls and that cache changes the answer.
    pristine: Model,
    tokenizer: Tokenizer,
    device: Device,
}

impl EmbeddingRuntime {
    /// Loads strictly from local files the config already hash-verified. This
    /// never reaches the network: candle and tokenizers are handed paths, never
    /// repository ids, so there is no code path that could fetch anything.
    pub fn load(store: &Path, config: &VectorConfig) -> Result<Self, Error> {
        let embedding = &config.embedding;
        if embedding.model_id != EMBED_MODEL_ID {
            return Err(vector_error("unsupported embedding model id"));
        }
        if config.dimensions != VECTOR_DIMENSIONS {
            return Err(vector_error("configured dimensions do not match the model"));
        }
        let qwen: Config = serde_json::from_slice(&std::fs::read(resolve(
            store,
            &embedding.model_config.path,
        ))?)?;
        if qwen.hidden_size != VECTOR_DIMENSIONS as usize {
            return Err(vector_error("model config declares other dimensions"));
        }
        let mut tokenizer = Tokenizer::from_file(resolve(store, &embedding.tokenizer.path))
            .map_err(|_| vector_error("tokenizer artifact is unreadable"))?;
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: MAX_TOKENS,
                ..TruncationParams::default()
            }))
            .map_err(|_| vector_error("tokenizer rejected the sequence limit"))?;
        let device = Device::Cpu;
        let pristine = Model::new(&qwen, weights(store, &embedding.model.path, &device)?)
            .map_err(|_| vector_error("model artifact does not load as Qwen3"))?;
        Ok(Self {
            descriptor: EmbeddingDescriptor {
                model_id: embedding.model_id.clone(),
                model_sha256: embedding.model.sha256.clone(),
                tokenizer_sha256: embedding.tokenizer.sha256.clone(),
                dimensions: config.dimensions,
                distance: config.distance,
                input_schema: embedding.input_schema.clone(),
            },
            pristine,
            tokenizer,
            device,
        })
    }

    /// A query carries the model's retrieval instruction; a stored document
    /// carries none. The model card is explicit that the document prompt is
    /// empty, so adding one to both halves would degrade ranking.
    pub fn embed_query(&self, query: &str) -> Result<Vec<f32>, Error> {
        self.forward(&format!("{QUERY_PREFIX}{query}"))
    }

    /// Each call runs on its own clone of the loaded model. Cloning shares the
    /// weight tensors and costs microseconds, but it hands the call an empty
    /// key/value cache — without which the vector for a record would depend on
    /// whatever was embedded before it, and drift silently through the index.
    fn forward(&self, text: &str) -> Result<Vec<f32>, Error> {
        let encoded = self
            .tokenizer
            .encode(text, true)
            .map_err(|_| vector_error("text could not be tokenized"))?;
        let ids = encoded.get_ids();
        if ids.is_empty() {
            return Err(vector_error("text produced no tokens"));
        }
        let mut model = self.pristine.clone();
        let input = Tensor::new(ids, &self.device)
            .and_then(|value| value.unsqueeze(0))
            .map_err(tensor_error)?;
        let hidden = model.forward(&input, 0).map_err(tensor_error)?;
        // Last-token pooling followed by L2 normalization, per the model's own
        // pooling and modules descriptors.
        let last = hidden.dim(1).map_err(tensor_error)? - 1;
        let pooled = hidden
            .i((.., last))
            .and_then(|value| value.to_dtype(DType::F32))
            .map_err(tensor_error)?;
        let length = pooled
            .sqr()
            .and_then(|value| value.sum_keepdim(1))
            .and_then(|value| value.sqrt())
            .map_err(tensor_error)?;
        pooled
            .broadcast_div(&length)
            .and_then(|value| value.flatten_all())
            .and_then(|value| value.to_vec1::<f32>())
            .map_err(tensor_error)
    }
}

impl Embedder for EmbeddingRuntime {
    fn descriptor(&self) -> &EmbeddingDescriptor {
        &self.descriptor
    }

    fn embed(&self, documents: &[EmbeddingDocument]) -> Result<Vec<Vec<f32>>, Error> {
        documents
            .iter()
            .map(|document| self.forward(&document.text))
            .collect()
    }
}

/// Qwen3-Embedding ships the base model: its tensors carry no `model.` prefix
/// and are bfloat16, while candle's Qwen3 expects both. Renaming and casting
/// once at load keeps that adaptation out of every later call.
fn weights<'a>(store: &Path, path: &Path, device: &Device) -> Result<VarBuilder<'a>, Error> {
    let raw = candle_core::safetensors::load(resolve(store, path), device)
        .map_err(|_| vector_error("model artifact is not readable safetensors"))?;
    let mut tensors = HashMap::with_capacity(raw.len());
    for (name, tensor) in raw {
        let tensor = tensor.to_dtype(DType::F32).map_err(tensor_error)?;
        tensors.insert(format!("model.{name}"), tensor);
    }
    Ok(VarBuilder::from_tensors(tensors, DType::F32, device))
}

fn tensor_error(_: candle_core::Error) -> Error {
    vector_error("embedding tensor operation failed")
}

fn resolve(store: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        store.join(path)
    }
}

#[cfg(test)]
mod tests;
