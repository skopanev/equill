//! The one way out of this process, and the settings that keep it that way.
//!
//! Separated from the provider so the provider's own rules — what it sends,
//! what it accepts back, what it says when the answer is wrong — can be tested
//! without a network. What cannot be tested here is the socket itself, and that
//! is exactly what this file is: configuration, no decisions.
use super::super::super::super::model::vector_error;
use crate::kernel::error::Error;
use std::time::Duration;
use ureq::Agent;

/// Where a Voyage embedding is asked for. Fixed, not configured: an endpoint an
/// operator can set is an endpoint an attacker who can write the descriptor can
/// point at themselves, and the key travels with the request.
pub(super) const ENDPOINT: &str = "https://api.voyageai.com/v1/embeddings";

/// A prompt-time hook waits behind this. Long enough for a batch of eight
/// against a hosted model, short enough that a hung call fails instead of
/// sitting in front of an agent's first turn.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// An embedding answer for eight vectors of 2048 floats is well under this. The
/// cap exists so a provider that answers with something else — a proxy error
/// page, a stream that does not end — cannot be read into memory unbounded.
const MAX_BODY: u64 = 4 * 1024 * 1024;

/// A POST that returns the status and the body, and nothing about how it got
/// them. The provider decides what a status means; this only reports it.
pub(super) trait Post: Send + Sync {
    fn post_json(&self, key: &str, body: &str) -> Result<(u16, String), Error>;
}

pub(super) struct Https {
    agent: Agent,
}

impl Https {
    pub(super) fn new() -> Self {
        let agent: Agent = Agent::config_builder()
            .timeout_global(Some(REQUEST_TIMEOUT))
            // Plaintext is never an acceptable fallback for a request carrying
            // an API key.
            .https_only(true)
            // A redirect would take the key somewhere the endpoint constant did
            // not name. Erroring is the point: following it silently is what
            // makes a fixed endpoint stop being fixed.
            .max_redirects(0)
            .max_redirects_will_error(true)
            // ureq reads proxy settings from the environment by default, so a
            // proxy variable in an operator's shell would put a middlebox in
            // front of an authenticated call without anyone deciding to.
            .proxy(None)
            .build()
            .into();
        Self { agent }
    }
}

impl Post for Https {
    fn post_json(&self, key: &str, body: &str) -> Result<(u16, String), Error> {
        let response = self
            .agent
            .post(ENDPOINT)
            .header("authorization", &format!("Bearer {key}"))
            .header("content-type", "application/json")
            .send(body);
        // Both arms carry a status and a body, because a 4xx is an answer about
        // the request and the provider has to be able to tell 429 from 401.
        let mut response = match response {
            Ok(response) => response,
            Err(ureq::Error::StatusCode(status)) => return Ok((status, String::new())),
            // Nothing from the transport is repeated: a connection error can
            // carry the URL, and a URL is one mistake away from carrying a key.
            Err(_) => return Err(vector_error("voyage embedding request failed")),
        };
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .with_config()
            .limit(MAX_BODY)
            .read_to_string()
            .map_err(|_| vector_error("voyage embedding response could not be read"))?;
        Ok((status, body))
    }
}
