mod args;
mod authority;
mod commands;
mod progress;
mod resume;

pub use args::*;
pub use authority::{GrantCommand, OwnerCommand, ReaderCommand};
use clap::Parser;
pub use commands::*;
pub(crate) use progress::HumanVectorProgress;
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
