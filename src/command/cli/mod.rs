mod args;
mod commands;

pub use args::*;
use clap::Parser;
pub use commands::*;

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
