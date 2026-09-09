use super::VectorConfig;
use crate::kernel::{digest::sha256_hex, error::Error};
use crate::vector::model::{DistanceMetric, INPUT_SCHEMA, vector_error};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VoyageEmbeddingConfig {
    pub provider: VoyageProvider,
    pub model_id: String,
    pub input_schema: String,
    pub api_key_file: PathBuf,
    #[serde(default)]
    pub allow_remote: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VoyageProvider {
    Voyage,
}

impl VoyageEmbeddingConfig {
    pub(crate) fn validate(&self, config: &VectorConfig) -> Result<(), Error> {
        if !self.allow_remote {
            return Err(vector_error(
                "voyage requires explicit remote embedding opt-in",
            ));
        }
        if self.model_id != "voyage-4-large"
            || self.input_schema != INPUT_SCHEMA
            || config.dimensions != 2048
            || config.distance != DistanceMetric::Cosine
        {
            return Err(vector_error(
                "voyage requires voyage-4-large, 2048 dimensions and cosine",
            ));
        }
        if !self.api_key_file.is_absolute() {
            return Err(vector_error(
                "voyage API key requires an absolute file path",
            ));
        }
        Ok(())
    }

    /// Hosted weights/tokenizer are not downloadable or hash-verifiable. This
    /// hash identifies the API/preprocessing contract, NOT pinned model weights.
    /// A provider-side model revision still requires an operator-led rebuild.
    pub(crate) fn fingerprint(&self) -> String {
        sha256_hex(format!(
            "equill.voyage.embedding.v1\nhttps://api.voyageai.com/v1/embeddings\n{}\n{}\n2048\ncosine\nfloat\nquery/document\ntruncation=false\n",
            self.model_id, self.input_schema,
        ).as_bytes())
    }
}

#[cfg(test)]
mod tests;
