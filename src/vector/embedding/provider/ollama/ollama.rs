use super::super::super::super::config::{OllamaEmbeddingConfig, VectorConfig};
use super::super::super::super::embedder::Embedder;
use super::super::super::super::model::{
    EmbeddingDescriptor, EmbeddingDocument, validate_vector, vector_error,
};
use super::super::super::QUERY_PREFIX;
use crate::kernel::error::Error;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use ureq::Agent;

const KEEP_ALIVE: i64 = -1;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const API_BATCH: usize = 8;

pub(in crate::vector::embedding) struct OllamaRuntime {
    descriptor: EmbeddingDescriptor,
    endpoint: String,
    model_id: String,
    dimensions: usize,
    agent: Agent,
}

#[derive(Deserialize)]
struct TagsResponse {
    models: Vec<ModelTag>,
}

#[derive(Deserialize)]
struct ModelTag {
    name: String,
    model: String,
    digest: String,
}

#[derive(Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: &'a [String],
    truncate: bool,
    dimensions: usize,
    keep_alive: i64,
}

#[derive(Deserialize)]
struct EmbedResponse {
    embeddings: Vec<Vec<f32>>,
}

impl OllamaRuntime {
    pub(in crate::vector::embedding) fn load(
        config: &VectorConfig,
        embedding: &OllamaEmbeddingConfig,
    ) -> Result<Self, Error> {
        let agent: Agent = Agent::config_builder()
            .timeout_global(Some(REQUEST_TIMEOUT))
            .build()
            .into();
        let endpoint = embedding.endpoint.trim_end_matches('/').to_owned();
        verify_model(&agent, &endpoint, embedding)?;
        Ok(Self {
            descriptor: EmbeddingDescriptor {
                model_id: embedding.model_id.clone(),
                model_sha256: embedding.model_sha256.clone(),
                tokenizer_sha256: embedding.model_sha256.clone(),
                dimensions: config.dimensions,
                distance: config.distance,
                input_schema: embedding.input_schema.clone(),
            },
            endpoint,
            model_id: embedding.model_id.clone(),
            dimensions: config.dimensions as usize,
            agent,
        })
    }

    pub(in crate::vector::embedding) fn embed_query(&self, query: &str) -> Result<Vec<f32>, Error> {
        let inputs = [format!("{QUERY_PREFIX}{query}")];
        self.request(&inputs)?
            .pop()
            .ok_or_else(|| vector_error("ollama returned no query embedding"))
    }

    fn request(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, Error> {
        let url = format!("{}/api/embed", self.endpoint);
        let mut response = self
            .agent
            .post(&url)
            .send_json(&EmbedRequest {
                model: &self.model_id,
                input: inputs,
                truncate: false,
                dimensions: self.dimensions,
                keep_alive: KEEP_ALIVE,
            })
            .map_err(|_| vector_error("ollama embedding request failed"))?;
        let body: EmbedResponse = response
            .body_mut()
            .read_json()
            .map_err(|_| vector_error("ollama embedding response is invalid"))?;
        if body.embeddings.len() != inputs.len() {
            return Err(vector_error("ollama returned the wrong batch size"));
        }
        for vector in &body.embeddings {
            validate_vector(vector, self.dimensions as u64)?;
        }
        Ok(body.embeddings)
    }
}

impl Embedder for OllamaRuntime {
    fn descriptor(&self) -> &EmbeddingDescriptor {
        &self.descriptor
    }

    fn embed(&self, documents: &[EmbeddingDocument]) -> Result<Vec<Vec<f32>>, Error> {
        let mut vectors = Vec::with_capacity(documents.len());
        for chunk in documents.chunks(API_BATCH) {
            let inputs = chunk
                .iter()
                .map(|document| document.text.clone())
                .collect::<Vec<_>>();
            vectors.extend(self.request(&inputs)?);
        }
        Ok(vectors)
    }
}

fn verify_model(
    agent: &Agent,
    endpoint: &str,
    embedding: &OllamaEmbeddingConfig,
) -> Result<(), Error> {
    let mut response = agent
        .get(&format!("{endpoint}/api/tags"))
        .call()
        .map_err(|_| vector_error("ollama is unavailable"))?;
    let body: TagsResponse = response
        .body_mut()
        .read_json()
        .map_err(|_| vector_error("ollama model inventory is invalid"))?;
    let found = body
        .models
        .into_iter()
        .find(|model| model.name == embedding.model_id || model.model == embedding.model_id);
    let Some(found) = found else {
        return Err(vector_error("configured ollama model is not installed"));
    };
    if found.digest.trim_start_matches("sha256:") != embedding.model_sha256 {
        return Err(vector_error("ollama model digest does not match config"));
    }
    Ok(())
}
