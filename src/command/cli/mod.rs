mod args;
mod audit;
mod authority;
mod commands;
mod format;
mod presentation;
mod progress;
mod record;
mod resume;

pub use args::*;
pub use audit::AuditCommand;
pub use authority::{GrantCommand, OwnerCommand, ReaderCommand};
use clap::Parser;
pub use commands::*;
pub use format::RecordFormatArg;
pub use presentation::{PresentationArgs, RetrievalArgs, RetrievalSourceArg};
pub(crate) use progress::HumanVectorProgress;
pub use record::RecordArgs;
pub use resume::held_to_reading;

#[derive(Debug, Parser)]
#[command(name = "equill", version, about)]
#[command(after_help = ACTOR_HELP)]
pub struct Cli {
    /// Emit stable machine-readable JSON.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}
