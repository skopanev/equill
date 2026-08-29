mod args;
mod commands;
mod progress;

pub use args::*;
use clap::Parser;
pub use commands::*;
pub(crate) use progress::HumanVectorProgress;

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
