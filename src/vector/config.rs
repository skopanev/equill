use super::model::{
    DistanceMetric, INPUT_SCHEMA, valid_collection_name, valid_sha256, vector_error,
};
use crate::kernel::error::Error;
use serde::{Deserialize, Serialize};
use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub(super) const CONFIG: &str = "registry/vector/qdrant.json";
const SCHEMA: &str = "equill.qdrant-config.v1";

mod artifact;
mod deepinfra;
mod embedding;
mod limits;
mod voyage;
use artifact::verify_artifact;
pub use deepinfra::{DeepInfraEmbeddingConfig, DeepInfraProvider};
pub use limits::DEFAULT_MAX_CHARS;
pub use voyage::{VoyageEmbeddingConfig, VoyageProvider};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VectorConfig {
    pub schema: String,
    pub enabled: bool,
    pub endpoint: String,
    pub collection_alias: String,
    pub store_id: Uuid,
    pub dimensions: u64,
    pub distance: DistanceMetric,
    pub embedding: EmbeddingConfig,
    /// Types this store embeds; empty means all of them. See `coverage`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub embed_types: Vec<String>,
    /// The most characters an embedding input may carry, in Unicode scalar
    /// values so a cap never splits a character. Two limits because the inputs
    /// differ: a question is typed, a document is what a record means. Absent
    /// means the default, not "no limit" — see `limits`.
    #[serde(default = "limits::default_chars")]
    pub max_query_chars: usize,
    #[serde(default = "limits::default_chars")]
    pub max_document_chars: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub allow_remote: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum EmbeddingConfig {
    DeepInfra(DeepInfraEmbeddingConfig),
    Voyage(VoyageEmbeddingConfig),
    Ollama(OllamaEmbeddingConfig),
    Local(LocalEmbeddingConfig),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LocalEmbeddingConfig {
    pub model_id: String,
    pub input_schema: String,
    pub model: ModelArtifact,
    pub tokenizer: ModelArtifact,
    // The Candle loader builds the network from the model's own config.json,
    // so it is a third hash-verified artifact rather than a file trusted by
    // position.
    #[serde(rename = "config")]
    pub model_config: ModelArtifact,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OllamaEmbeddingConfig {
    pub provider: OllamaProvider,
    pub endpoint: String,
    pub model_id: String,
    pub model_sha256: String,
    pub input_schema: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OllamaProvider {
    Ollama,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelArtifact {
    pub path: PathBuf,
    pub sha256: String,
}

pub(crate) fn load(store: &Path) -> Result<Option<VectorConfig>, Error> {
    let path = store.join(CONFIG);
    if !path.is_file() {
        return Ok(None);
    }
    let config: VectorConfig = serde_json::from_slice(&fs::read(path)?)?;
    validate_shape(&config)?;
    if config.enabled
        && let EmbeddingConfig::Local(embedding) = &config.embedding
    {
        verify_artifact(store, &embedding.model, "model")?;
        verify_artifact(store, &embedding.tokenizer, "tokenizer")?;
        verify_artifact(store, &embedding.model_config, "model config")?;
    }
    Ok(Some(config))
}

impl VectorConfig {
    pub(crate) fn api_key(&self) -> Result<Option<String>, Error> {
        self.api_key_env
            .as_ref()
            .map(|name| {
                std::env::var(name)
                    .map_err(|_| vector_error("configured API key environment variable is missing"))
            })
            .transpose()
    }
}

fn validate_shape(config: &VectorConfig) -> Result<(), Error> {
    if config.schema != SCHEMA {
        return Err(vector_error("unsupported config schema"));
    }
    validate_endpoint(&config.endpoint, config.allow_remote)?;
    if !valid_collection_name(&config.collection_alias) {
        return Err(vector_error("invalid collection alias"));
    }
    limits::validate(config.max_query_chars, "max_query_chars")?;
    limits::validate(config.max_document_chars, "max_document_chars")?;
    if !(1..=65_536).contains(&config.dimensions) {
        return Err(vector_error("dimensions must be between 1 and 65536"));
    }
    super::coverage::validate_names(&config.embed_types)?;
    if config.embedding.model_id().trim().is_empty()
        || config.embedding.input_schema() != INPUT_SCHEMA
    {
        return Err(vector_error("invalid embedding descriptor"));
    }
    match &config.embedding {
        EmbeddingConfig::DeepInfra(embedding) => embedding.validate(config)?,
        EmbeddingConfig::Voyage(embedding) => embedding.validate(config)?,
        EmbeddingConfig::Local(embedding) => {
            for artifact in [
                &embedding.model,
                &embedding.tokenizer,
                &embedding.model_config,
            ] {
                if artifact.path.as_os_str().is_empty() || !valid_sha256(&artifact.sha256) {
                    return Err(vector_error(
                        "model artifacts require local paths and SHA-256",
                    ));
                }
            }
        }
        EmbeddingConfig::Ollama(embedding) => {
            validate_endpoint(&embedding.endpoint, false)?;
            if !valid_sha256(&embedding.model_sha256) {
                return Err(vector_error(
                    "ollama model requires a pinned SHA-256 digest",
                ));
            }
        }
    }
    if config.api_key_env.as_deref().is_some_and(|name| {
        name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    }) {
        return Err(vector_error("invalid API key environment variable name"));
    }
    Ok(())
}

fn validate_endpoint(endpoint: &str, allow_remote: bool) -> Result<(), Error> {
    let (secure, authority) = endpoint
        .strip_prefix("https://")
        .map(|value| (true, value))
        .or_else(|| endpoint.strip_prefix("http://").map(|value| (false, value)))
        .ok_or_else(|| vector_error("endpoint must use http or https"))?;
    if authority.contains(['/', '?', '#', '@']) {
        return Err(vector_error("endpoint must contain only host and port"));
    }
    let (host, port) = split_authority(authority)?;
    let local = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    if !local && (!allow_remote || !secure) {
        return Err(vector_error("remote endpoint requires explicit TLS opt-in"));
    }
    if port.parse::<u16>().is_err() {
        return Err(vector_error("endpoint requires a valid port"));
    }
    Ok(())
}

fn split_authority(authority: &str) -> Result<(&str, &str), Error> {
    if let Some(rest) = authority.strip_prefix('[') {
        let (host, port) = rest
            .split_once("]:")
            .ok_or_else(|| vector_error("invalid IPv6 endpoint"))?;
        return Ok((host, port));
    }
    authority
        .rsplit_once(':')
        .filter(|(host, port)| !host.is_empty() && !port.is_empty())
        .ok_or_else(|| vector_error("endpoint requires host and port"))
}
