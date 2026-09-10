//! Embeddings from a hosted Qwen3-Embedding-8B, over one fixed HTTPS endpoint.
//!
//! The difference from the other hosted provider is worth stating, because the
//! two do the opposite thing and both are right. Voyage asks which kind of text
//! it is being given, through an `input_type` field, so the local models'
//! instruction prefix must NOT be repeated there. DeepInfra's API has no such
//! field — `model`, `input`, `encoding_format` and nothing else — so the
//! distinction between a question and a document exists only in the text we
//! send, and Qwen's own model card gives the template:
//!
//!   query:    `Instruct: {task}\nQuery:{query}`  (no space after `Query:`)
//!   document: no prefix at all
//!
//! `embedding::instructed_query` already builds exactly that string, so this
//! provider uses it and Voyage ignores it.
//!
//! As with any hosted model the weights cannot be hashed, so the descriptor
//! identifies the API and preprocessing contract instead; a provider-side
//! revision is an operator-led rebuild, not something this can detect. And
//! every call carries a credential, which is why the key is read at the moment
//! it is used and never lives in this struct: a field nobody holds cannot be
//! printed by a future `Debug`, logged by a future diagnostic, or copied into
//! an error message by accident.
use super::super::super::super::config::{DeepInfraEmbeddingConfig, VectorConfig};
use super::super::super::super::embedder::Embedder;
use super::super::super::super::model::{
    EmbeddingDescriptor, EmbeddingDocument, validate_vector, vector_error,
};
use super::super::super::instructed_query;
use super::transport::{Https, Post};
use crate::kernel::error::Error;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Conservative and ours, not the provider's: DeepInfra documents no batch
/// limit for this model. Small keeps a failed batch cheap to retry and a rate
/// limit cheap to hit, against a model whose context is 32k tokens per input.
const API_BATCH: usize = 8;

pub(in crate::vector::embedding) struct DeepInfraRuntime {
    descriptor: EmbeddingDescriptor,
    model_id: String,
    api_key_file: PathBuf,
    transport: Box<dyn Post>,
}

#[derive(Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: &'a [String],
    /// The only optional field the API documents, and only `float` is
    /// documented for it. `dimensions` is NOT sent: DeepInfra does not document
    /// such a parameter for this endpoint, so the width is the model's native
    /// 4096 — checked on the way back rather than asked for on the way out.
    encoding_format: &'a str,
}

#[derive(Deserialize)]
struct EmbedResponse {
    model: String,
    data: Vec<EmbedDatum>,
}

#[derive(Deserialize)]
struct EmbedDatum {
    embedding: Vec<f32>,
    index: usize,
}

impl DeepInfraRuntime {
    pub(in crate::vector::embedding) fn load(
        config: &VectorConfig,
        embedding: &DeepInfraEmbeddingConfig,
    ) -> Result<Self, Error> {
        Self::with_transport(config, embedding, Box::new(Https::new()))
    }

    fn with_transport(
        config: &VectorConfig,
        embedding: &DeepInfraEmbeddingConfig,
        transport: Box<dyn Post>,
    ) -> Result<Self, Error> {
        // The same check the config reader runs. Asked again here because a
        // runtime that trusts its caller to have validated is a runtime that
        // stops being safe the first time someone calls it from somewhere else.
        embedding.validate(config)?;
        let fingerprint = embedding.fingerprint();
        Ok(Self {
            descriptor: EmbeddingDescriptor {
                model_id: embedding.model_id.clone(),
                // Not weights. Both slots carry the contract hash because there
                // is no tokenizer to name either: the provider tokenizes, and
                // what this store can pin is what it asked for.
                model_sha256: fingerprint.clone(),
                tokenizer_sha256: fingerprint,
                dimensions: config.dimensions,
                distance: config.distance,
                input_schema: embedding.input_schema.clone(),
            },
            model_id: embedding.model_id.clone(),
            api_key_file: embedding.api_key_file.clone(),
            transport,
        })
    }

    /// The query carries Qwen's instruction prefix, because nothing else can
    /// tell this provider that it is a question. Sending it bare would embed it
    /// in the document space and quietly cost recall.
    pub(in crate::vector::embedding) fn embed_query(
        &self,
        instruction: &str,
        query: &str,
    ) -> Result<Vec<f32>, Error> {
        let inputs = [instructed_query(instruction, query)];
        self.request(&inputs)?
            .pop()
            .ok_or_else(|| vector_error("deepinfra returned no query embedding"))
    }

    /// The key, read now rather than held.
    ///
    /// Errors name the failure and never the file's contents. A key that will
    /// not load is an operator problem, and the operator has the path.
    fn api_key(&self) -> Result<String, Error> {
        let raw = super::key::read_bounded(&self.api_key_file)?;
        let key = raw.trim();
        if key.is_empty() {
            return Err(vector_error("deepinfra API key file is empty"));
        }
        // A newline or a space would end up in a header, and a malformed header
        // fails as an authentication error, which sends the operator looking at
        // the wrong thing.
        if key.bytes().any(|byte| !(0x21..=0x7e).contains(&byte)) {
            return Err(vector_error(
                "deepinfra API key file contains characters a header cannot carry",
            ));
        }
        Ok(key.to_owned())
    }

    fn request(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, Error> {
        let body = serde_json::to_string(&EmbedRequest {
            model: &self.model_id,
            input: inputs,
            encoding_format: "float",
        })
        .map_err(|_| vector_error("deepinfra embedding request could not be encoded"))?;
        let key = self.api_key()?;
        // One call, no retry. A hidden retry against a paid endpoint doubles a
        // bill and hides a rate limit from the operator who has to act on it.
        let (status, body) = self.transport.post_json(&key, &body)?;
        if status != 200 {
            return Err(status_error(status));
        }
        self.parse(&body, inputs.len())
    }

    fn parse(&self, body: &str, expected: usize) -> Result<Vec<Vec<f32>>, Error> {
        let response: EmbedResponse = serde_json::from_str(body)
            .map_err(|_| vector_error("deepinfra embedding response is invalid"))?;
        // A different model is a different vector space. Silently mixing two of
        // them in one collection produces distances that mean nothing, and
        // nothing downstream could notice.
        if response.model != self.model_id {
            return Err(vector_error("deepinfra answered with a different model"));
        }
        if response.data.len() != expected {
            return Err(vector_error("deepinfra returned the wrong batch size"));
        }
        // Ordered by index rather than by position: the batch is zipped against
        // the documents that produced it, so a reordered response would attach
        // each vector to the wrong record — a corruption no later check sees.
        let mut vectors = vec![None; expected];
        for datum in response.data {
            let slot = vectors
                .get_mut(datum.index)
                .ok_or_else(|| vector_error("deepinfra returned an out-of-range index"))?;
            if slot.is_some() {
                return Err(vector_error("deepinfra returned a repeated index"));
            }
            validate_vector(&datum.embedding, self.descriptor.dimensions)?;
            *slot = Some(datum.embedding);
        }
        vectors
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| vector_error("deepinfra left a gap in the batch"))
    }
}

/// What a status means, said without the provider's own words.
///
/// DeepInfra does not document an error body shape, and a body can repeat the
/// request, which carried a key. None of it is quoted back; the status is
/// enough to act on and cannot leak.
fn status_error(status: u16) -> Error {
    let reason = match status {
        401 | 403 => "deepinfra rejected the API key",
        402 => "deepinfra reports the account cannot be billed for this request",
        404 => "deepinfra does not offer the configured model",
        413 => "deepinfra refused the batch as too large",
        422 => "deepinfra refused the request as malformed",
        429 => "deepinfra rate limit reached; the pass keeps its checkpoint and can be run again",
        500..=599 => "deepinfra is unavailable",
        _ => "deepinfra refused the embedding request",
    };
    vector_error(reason)
}

impl Embedder for DeepInfraRuntime {
    fn descriptor(&self) -> &EmbeddingDescriptor {
        &self.descriptor
    }

    /// Documents go bare: Qwen's card is explicit that retrieval documents take
    /// no instruction, and adding one would put them in a different space from
    /// every document embedded before it.
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

#[cfg(test)]
pub(super) fn for_tests(
    config: &VectorConfig,
    embedding: &DeepInfraEmbeddingConfig,
    transport: Box<dyn Post>,
) -> Result<DeepInfraRuntime, Error> {
    DeepInfraRuntime::with_transport(config, embedding, transport)
}
