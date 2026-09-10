use super::VectorConfig;
use crate::kernel::{digest::sha256_hex, error::Error};
use crate::vector::model::{DistanceMetric, INPUT_SCHEMA, vector_error};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeepInfraEmbeddingConfig {
    pub provider: DeepInfraProvider,
    pub model_id: String,
    pub input_schema: String,
    pub api_key_file: PathBuf,
    #[serde(default)]
    pub allow_remote: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeepInfraProvider {
    Deepinfra,
}

impl DeepInfraEmbeddingConfig {
    pub(crate) fn validate(&self, config: &VectorConfig) -> Result<(), Error> {
        if !self.allow_remote {
            return Err(vector_error(
                "deepinfra requires explicit remote embedding opt-in",
            ));
        }
        // 4096 is the model's native width. DeepInfra documents no `dimensions`
        // parameter, so this is the only width we can ask for — by not asking.
        if self.model_id != "Qwen/Qwen3-Embedding-8B"
            || self.input_schema != INPUT_SCHEMA
            || config.dimensions != 4096
            || config.distance != DistanceMetric::Cosine
        {
            return Err(vector_error(
                "deepinfra requires Qwen/Qwen3-Embedding-8B, 4096 dimensions and cosine",
            ));
        }
        if !self.api_key_file.is_absolute() {
            return Err(vector_error(
                "deepinfra API key requires an absolute file path",
            ));
        }
        Ok(())
    }

    /// Hosted weights and tokenizer are not downloadable or hash-verifiable.
    /// This hash identifies the API and preprocessing contract, NOT pinned
    /// model weights: a provider-side revision of the model would not change
    /// it, and catching that still needs an operator-led rebuild.
    ///
    /// `instruct/bare` names the preprocessing this provider requires and is
    /// part of the contract: a query carries Qwen's instruction prefix and a
    /// document carries none. Changing that changes the vector space, so it
    /// must change the fingerprint.
    pub(crate) fn fingerprint(&self) -> String {
        sha256_hex(
            format!(
                "equill.deepinfra.embedding.v1\nhttps://api.deepinfra.com/v1/openai/embeddings\n{}\n{}\n4096\ncosine\nfloat\ninstruct/bare\n",
                self.model_id, self.input_schema,
            )
            .as_bytes(),
        )
    }
}
