//! What every embedding descriptor can be asked, whichever provider it names.
//!
//! The two hosted providers answer both digest questions with the same
//! contract fingerprint, because hosted weights cannot be hashed and there is
//! no separate tokenizer to name: what a store can pin is what it asked for.
use super::EmbeddingConfig;

impl EmbeddingConfig {
    pub(crate) fn model_id(&self) -> &str {
        match self {
            Self::DeepInfra(value) => &value.model_id,
            Self::Voyage(value) => &value.model_id,
            Self::Local(value) => &value.model_id,
            Self::Ollama(value) => &value.model_id,
        }
    }

    pub(crate) fn model_sha256(&self) -> String {
        match self {
            Self::DeepInfra(value) => value.fingerprint(),
            Self::Voyage(value) => value.fingerprint(),
            Self::Local(value) => value.model.sha256.clone(),
            Self::Ollama(value) => value.model_sha256.clone(),
        }
    }

    pub(crate) fn tokenizer_sha256(&self) -> String {
        match self {
            Self::DeepInfra(value) => value.fingerprint(),
            Self::Voyage(value) => value.fingerprint(),
            Self::Local(value) => value.tokenizer.sha256.clone(),
            Self::Ollama(value) => value.model_sha256.clone(),
        }
    }

    pub(crate) fn input_schema(&self) -> &str {
        match self {
            Self::DeepInfra(value) => &value.input_schema,
            Self::Voyage(value) => &value.input_schema,
            Self::Local(value) => &value.input_schema,
            Self::Ollama(value) => &value.input_schema,
        }
    }
}
