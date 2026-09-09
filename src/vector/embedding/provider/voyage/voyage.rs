//! Embeddings from a hosted model, asked for over one fixed HTTPS endpoint.
//!
//! Two things separate this from the local providers. The weights cannot be
//! hashed, so the descriptor identifies the API and preprocessing contract
//! instead — a provider-side revision is an operator-led rebuild, not something
//! this can detect. And every call carries a credential, which is why the key
//! is read at the moment it is used and never lives in this struct: a field
//! nobody holds cannot be printed by a future `Debug`, logged by a future
//! diagnostic, or copied into an error message by accident.
use super::super::super::super::config::{VectorConfig, VoyageEmbeddingConfig};
use super::super::super::super::embedder::Embedder;
use super::super::super::super::model::{
    EmbeddingDescriptor, EmbeddingDocument, validate_vector, vector_error,
};
use super::transport::{Https, Post};
use crate::kernel::error::Error;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// What the provider accepts in one call. Eight keeps a failed batch cheap to
/// retry and a rate limit cheap to hit.
const API_BATCH: usize = 8;

pub(in crate::vector::embedding) struct VoyageRuntime {
    descriptor: EmbeddingDescriptor,
    model_id: String,
    api_key_file: PathBuf,
    transport: Box<dyn Post>,
}

#[derive(Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: &'a [String],
    /// `query` and `document` are different embeddings of the same words, and
    /// the provider needs to be told which is being asked for.
    input_type: &'a str,
    truncation: bool,
    output_dtype: &'a str,
    /// Sent explicitly, never left to the provider. The default is 1024, the
    /// collection is 2048, and a silent default would fail every call after the
    /// configuration had already been accepted as valid.
    output_dimension: u64,
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

impl VoyageRuntime {
    pub(in crate::vector::embedding) fn load(
        config: &VectorConfig,
        embedding: &VoyageEmbeddingConfig,
    ) -> Result<Self, Error> {
        Self::with_transport(config, embedding, Box::new(Https::new()))
    }

    fn with_transport(
        config: &VectorConfig,
        embedding: &VoyageEmbeddingConfig,
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

    /// The query goes as the caller wrote it.
    ///
    /// `instruction` is the local models' prefix — "Instruct: …\nQuery:…" is how
    /// Qwen was told what kind of similarity to look for. Voyage asks for the
    /// same thing through `input_type`, so repeating the prefix here would
    /// embed the instruction as part of the question and pull every answer
    /// toward the words of the instruction rather than the words of the query.
    pub(in crate::vector::embedding) fn embed_query(
        &self,
        _instruction: &str,
        query: &str,
    ) -> Result<Vec<f32>, Error> {
        let inputs = [query.to_owned()];
        self.request(&inputs, "query")?
            .pop()
            .ok_or_else(|| vector_error("voyage returned no query embedding"))
    }

    /// The key, read now rather than held.
    ///
    /// Errors name the failure and never the file's contents. A key that will
    /// not load is an operator problem, and the operator has the path.
    fn api_key(&self) -> Result<String, Error> {
        // Bounded before it is read. A path that points at something enormous —
        // a log, a device, a mistake — must fail as a bad key file rather than
        // as a process that grew until it died.
        let raw = read_bounded(&self.api_key_file)?;
        let key = raw.trim();
        if key.is_empty() {
            return Err(vector_error("voyage API key file is empty"));
        }
        // A newline or a space would end up in a header, and a malformed header
        // fails as an authentication error, which sends the operator looking at
        // the wrong thing.
        if key.bytes().any(|byte| !(0x21..=0x7e).contains(&byte)) {
            return Err(vector_error(
                "voyage API key file contains characters a header cannot carry",
            ));
        }
        Ok(key.to_owned())
    }

    fn request(&self, inputs: &[String], input_type: &str) -> Result<Vec<Vec<f32>>, Error> {
        let body = serde_json::to_string(&EmbedRequest {
            model: &self.model_id,
            input: inputs,
            input_type,
            truncation: false,
            output_dtype: "float",
            output_dimension: self.descriptor.dimensions,
        })
        .map_err(|_| vector_error("voyage embedding request could not be encoded"))?;
        let key = self.api_key()?;
        let (status, body) = self.transport.post_json(&key, &body)?;
        if status != 200 {
            return Err(status_error(status));
        }
        self.parse(&body, inputs.len())
    }

    fn parse(&self, body: &str, expected: usize) -> Result<Vec<Vec<f32>>, Error> {
        let response: EmbedResponse = serde_json::from_str(body)
            .map_err(|_| vector_error("voyage embedding response is invalid"))?;
        // A different model is a different vector space. Silently mixing two of
        // them in one collection produces distances that mean nothing, and
        // nothing downstream could notice.
        if response.model != self.model_id {
            return Err(vector_error("voyage answered with a different model"));
        }
        if response.data.len() != expected {
            return Err(vector_error("voyage returned the wrong batch size"));
        }
        // Ordered by index rather than by position: the batch is zipped against
        // the documents that produced it, so a reordered response would attach
        // each vector to the wrong record — a corruption no later check sees.
        let mut vectors = vec![None; expected];
        for datum in response.data {
            let slot = vectors
                .get_mut(datum.index)
                .ok_or_else(|| vector_error("voyage returned an out-of-range index"))?;
            if slot.is_some() {
                return Err(vector_error("voyage returned a repeated index"));
            }
            validate_vector(&datum.embedding, self.descriptor.dimensions)?;
            *slot = Some(datum.embedding);
        }
        vectors
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| vector_error("voyage left a gap in the batch"))
    }
}

/// The largest thing that can sensibly be an API key. Anything past this is a
/// pointed-at-the-wrong-file mistake, and reading it is the harm.
const MAX_KEY_BYTES: usize = 8192;

fn read_bounded(path: &std::path::Path) -> Result<String, Error> {
    use std::io::Read;
    let mut file =
        fs::File::open(path).map_err(|_| vector_error("voyage API key file could not be read"))?;
    let mut buffer = Vec::new();
    file.by_ref()
        .take(MAX_KEY_BYTES as u64 + 1)
        .read_to_end(&mut buffer)
        .map_err(|_| vector_error("voyage API key file could not be read"))?;
    if buffer.len() > MAX_KEY_BYTES {
        return Err(vector_error("voyage API key file is too large to be a key"));
    }
    String::from_utf8(buffer).map_err(|_| vector_error("voyage API key file is not text"))
}

/// What a status means, said without the provider's own words.
///
/// A response body can repeat the request, and the request carried a key. None
/// of it is quoted back; the status is enough to act on and cannot leak.
fn status_error(status: u16) -> Error {
    let reason = match status {
        401 | 403 => "voyage rejected the API key",
        404 => "voyage does not offer the configured model",
        413 => "voyage refused the batch as too large",
        422 => "voyage refused the request as malformed",
        429 => "voyage rate limit reached; the pass keeps its checkpoint and can be run again",
        500..=599 => "voyage is unavailable",
        _ => "voyage refused the embedding request",
    };
    vector_error(reason)
}

impl Embedder for VoyageRuntime {
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
            vectors.extend(self.request(&inputs, "document")?);
        }
        Ok(vectors)
    }
}

#[cfg(test)]
pub(super) fn for_tests(
    config: &VectorConfig,
    embedding: &VoyageEmbeddingConfig,
    transport: Box<dyn Post>,
) -> Result<VoyageRuntime, Error> {
    VoyageRuntime::with_transport(config, embedding, transport)
}
