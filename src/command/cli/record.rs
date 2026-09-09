use clap::Args;
use std::path::PathBuf;

#[derive(Debug, Args)]
pub struct RecordArgs {
    /// Initialized store directory.
    #[arg(long)]
    pub store: PathBuf,
    /// Record draft JSON or JSONL input. Actor comes from EQUILL_ACTOR.
    #[arg(long)]
    pub input: PathBuf,
    /// Opaque identity for retrying one append. JSONL entries carry their own keys.
    #[arg(long)]
    pub idempotency_key: Option<String>,
}
