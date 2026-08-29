//! The argument vocabularies the command list refers to, kept apart so the
//! list itself stays readable.
use clap::{Subcommand, ValueEnum};

/// The first thing a new caller trips over, so it belongs in `--help` rather
/// than in an error message they have to provoke.
pub const ACTOR_HELP: &str = concat!(
    "Actor:\n",
    "  Every write and every context assembly reads EQUILL_ACTOR from the\n",
    "  environment. Set it to an identity the store knows: its root owner, an\n",
    "  actor listed in the store's writers, or one covered by a write grant.\n",
    "  An unset or unknown actor fails the call before anything is read."
);

use std::path::PathBuf;

#[derive(Debug, Subcommand)]
pub enum SchemaCommand {
    /// List the types this store has registered.
    List {
        /// Initialized store directory.
        #[arg(long)]
        store: PathBuf,
    },
    /// Show one type: its fields, which are required, and the legal values of
    /// any constrained field.
    Show {
        /// Initialized store directory.
        #[arg(long)]
        store: PathBuf,
        /// Registered type name.
        #[arg(long = "type")]
        type_name: String,
    },
    /// Register one immutable versioned type definition.
    Register {
        /// Initialized store directory.
        #[arg(long)]
        store: PathBuf,
        /// Type definition JSON file.
        #[arg(long)]
        file: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub enum RegistryCommand {
    /// Register one immutable profile or selector definition.
    Register {
        /// Initialized store directory.
        #[arg(long)]
        store: PathBuf,
        /// Definition JSON file.
        #[arg(long)]
        file: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub enum VectorCommand {
    /// Install or replace the store's vector configuration after validating it.
    Configure {
        /// Initialized store directory.
        #[arg(long)]
        store: PathBuf,
        /// Vector configuration JSON file.
        #[arg(long)]
        file: PathBuf,
    },
    /// Turn the vector projection off without discarding its descriptor.
    Disable {
        /// Initialized store directory.
        #[arg(long)]
        store: PathBuf,
    },
    /// Re-embed the immutable ledger and activate the result atomically.
    Rebuild {
        /// Initialized store directory.
        #[arg(long)]
        store: PathBuf,
    },
    /// Incrementally embed records missing from the active collection.
    Sync {
        /// Initialized store directory.
        #[arg(long)]
        store: PathBuf,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum StrategyArg {
    /// Full-text search over the embedded projection.
    Fts,
    /// Semantic search only; an unavailable vector index is an error.
    Vector,
    /// Semantic search with a reported fall back to full text.
    Hybrid,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum FormatArg {
    /// One JSON object per line: today's shape, and the one scripts expect.
    Jsonl,
    /// One readable line per record.
    Text,
}

#[cfg(test)]
mod tests {
    use super::super::{Cli, Command, VectorCommand};
    use clap::Parser;
    use std::path::PathBuf;

    #[test]
    fn context_accepts_named_coordinate_shortcuts() {
        let cli = Cli::try_parse_from([
            "equill",
            "context",
            "--store",
            "store",
            "--profile",
            "worker",
            "--query",
            "retry",
            "--project",
            "finik",
            "--role",
            "backend",
            "--phase",
            "unit",
            "--harness",
            "codex",
        ])
        .expect("named coordinates");

        let Command::Context {
            project,
            role,
            phase,
            harness,
            ..
        } = cli.command
        else {
            panic!("context command");
        };
        assert_eq!(project.as_deref(), Some("finik"));
        assert_eq!(role.as_deref(), Some("backend"));
        assert_eq!(phase.as_deref(), Some("unit"));
        assert_eq!(harness.as_deref(), Some("codex"));
    }

    #[test]
    fn vector_sync_accepts_an_explicit_store() {
        let cli = Cli::try_parse_from(["equill", "vector", "sync", "--store", "store"])
            .expect("vector sync");

        let Command::Vector {
            command: VectorCommand::Sync { store },
        } = cli.command
        else {
            panic!("vector sync command");
        };
        assert_eq!(store, PathBuf::from("store"));
    }
}
