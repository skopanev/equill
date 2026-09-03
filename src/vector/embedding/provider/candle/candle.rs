use super::super::super::super::config::{LocalEmbeddingConfig, VectorConfig};
use super::super::super::super::embedder::Embedder;
use super::super::super::super::model::{EmbeddingDescriptor, EmbeddingDocument, vector_error};
use super::super::super::QUERY_PREFIX;
use crate::kernel::error::Error;
use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::qwen3::{Config, Model};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokenizers::{Tokenizer, TruncationParams};

pub const EMBED_MODEL_ID: &str = "Qwen/Qwen3-Embedding-0.6B";
pub const VECTOR_DIMENSIONS: u64 = 1024;
pub const MAX_TOKENS: usize = 512;

pub(in crate::vector::embedding) struct CandleRuntime {
    descriptor: EmbeddingDescriptor,
    pristine: Model,
    tokenizer: Tokenizer,
    device: Device,
}

impl CandleRuntime {
    pub(in crate::vector::embedding) fn load(
        store: &Path,
        config: &VectorConfig,
        embedding: &LocalEmbeddingConfig,
    ) -> Result<Self, Error> {
        if embedding.model_id != EMBED_MODEL_ID {
            return Err(vector_error("unsupported local embedding model id"));
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

    pub(in crate::vector::embedding) fn embed_query(&self, query: &str) -> Result<Vec<f32>, Error> {
        self.forward(&format!("{QUERY_PREFIX}{query}"))
    }

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

impl Embedder for CandleRuntime {
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
