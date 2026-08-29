//! The argument vocabularies the command list refers to, kept apart so the
//! list itself stays readable.
use clap::{Subcommand, ValueEnum};
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
