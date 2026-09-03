// Provider layout intentionally keeps implementation in provider/<name>/<name>.rs.
#[allow(clippy::module_inception)]
mod candle;

pub(in crate::vector::embedding) use candle::CandleRuntime;
pub use candle::{EMBED_MODEL_ID, MAX_TOKENS, VECTOR_DIMENSIONS};
