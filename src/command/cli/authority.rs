//! The two governance vocabularies: who owns the store, and who may append to
//! part of it.
use super::args::ACTOR_HELP;
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Debug, Subcommand)]
pub enum OwnerCommand {
    /// Show the owner, the store-wide writers, and every scoped grant.
    Show {
        /// Initialized store directory.
        #[arg(long)]
        store: PathBuf,
    },
    /// Hand the store to a new root owner.
    #[command(after_help = ACTOR_HELP)]
    Transfer {
        /// Initialized store directory.
        #[arg(long)]
        store: PathBuf,
        /// Identity that becomes the root owner.
        #[arg(long)]
        to: String,
        /// Why the store is changing hands. Only its sha256 digest is stored;
        /// the text itself never enters the ledger.
        #[arg(long)]
        comment: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum GrantCommand {
    /// List the grants in force.
    List {
        /// Initialized store directory.
        #[arg(long)]
        store: PathBuf,
    },
    /// Allow one actor to append one namespace and type list.
    #[command(after_help = ACTOR_HELP)]
    Add {
        /// Initialized store directory.
        #[arg(long)]
        store: PathBuf,
        /// Identity the grant is written for.
        #[arg(long)]
        actor: String,
        /// Namespace the grant covers. `*` covers every namespace.
        #[arg(long)]
        namespace: String,
        /// Types the grant covers. `*` covers every type.
        #[arg(long, value_delimiter = ',')]
        types: Vec<String>,
        /// Why the grant is being written. Only its sha256 digest is stored;
        /// the text itself never enters the ledger.
        #[arg(long)]
        comment: Option<String>,
    },
    /// Withdraw every grant naming one actor.
    #[command(after_help = ACTOR_HELP)]
    Revoke {
        /// Initialized store directory.
        #[arg(long)]
        store: PathBuf,
        /// Identity to remove from the grants.
        #[arg(long)]
        actor: String,
        /// Why it is being withdrawn. Only its sha256 digest is stored; the
        /// text itself never enters the ledger.
        #[arg(long)]
        comment: Option<String>,
    },
}
