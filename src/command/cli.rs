use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "equill", version, about)]
pub struct Cli {
    /// Emit stable machine-readable JSON.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create a store, its root owner, and its first namespace.
    Init {
        /// Store directory to create.
        #[arg(long)]
        store: PathBuf,
        /// Stable identity of the store's root owner.
        #[arg(long)]
        owner: String,
        /// First logical namespace in the store.
        #[arg(long)]
        namespace: String,
    },
    /// Append one schema-validated immutable record.
    Record {
        /// Initialized store directory.
        #[arg(long)]
        store: PathBuf,
        /// Record draft JSON file. Actor comes from EQUILL_ACTOR.
        #[arg(long)]
        input: PathBuf,
    },
    /// Import a legacy JSONL batch through the canonical writer.
    Import {
        /// Initialized store directory.
        #[arg(long)]
        store: PathBuf,
        /// Legacy record envelope JSONL file.
        #[arg(
            long,
            required_unless_present = "manifest",
            conflicts_with = "manifest"
        )]
        input: Option<PathBuf>,
        /// JSONL manifest whose rows point to input JSONL files.
        #[arg(long, required_unless_present = "input", conflicts_with = "input")]
        manifest: Option<PathBuf>,
    },
    /// Preview or apply manifest-wide source compaction.
    Compact {
        /// Initialized store directory to rebuild after apply.
        #[arg(long)]
        store: PathBuf,
        /// Complete JSONL input manifest.
        #[arg(long)]
        manifest: PathBuf,
        /// Report removals without changing inputs or the store.
        #[arg(long, required_unless_present = "apply", conflicts_with = "apply")]
        dry_run: bool,
        /// Rewrite inputs and rebuild the store after validation.
        #[arg(long, required_unless_present = "dry_run", conflicts_with = "dry_run")]
        apply: bool,
    },
    /// Check executable and store health.
    Doctor {
        /// Store directory to inspect.
        #[arg(long)]
        store: Option<PathBuf>,
        /// Scan records, projections, and gates instead of quick health only.
        #[arg(long)]
        full: bool,
        /// Run the offline full-catalog memory-defense audit.
        #[arg(long, requires = "store", conflicts_with = "full")]
        deep: bool,
    },
    /// Manage governed record schemas.
    Schema {
        #[command(subcommand)]
        command: SchemaCommand,
    },
    /// Manage governed context profiles.
    Profile {
        #[command(subcommand)]
        command: RegistryCommand,
    },
    /// Manage canonical per-type selectors.
    Selector {
        #[command(subcommand)]
        command: RegistryCommand,
    },
    /// Assemble deterministic bounded context from one explicit store.
    Context {
        /// Initialized store directory.
        #[arg(long)]
        store: PathBuf,
        /// Registered context profile identifier.
        #[arg(long)]
        profile: String,
        /// Context request JSON file.
        #[arg(long)]
        request: PathBuf,
    },
    /// Show installed, configured, optional, and planned components.
    Status {
        /// Store directory to inspect.
        #[arg(long)]
        store: Option<PathBuf>,
    },
    /// Search the embedded full-text projection.
    Search {
        /// Initialized store directory.
        #[arg(long)]
        store: PathBuf,
        /// Text to match.
        #[arg(long)]
        query: String,
        /// Restrict matches to one namespace.
        #[arg(long)]
        namespace: Option<String>,
        /// Restrict matches to one registered type.
        #[arg(long = "type")]
        type_name: Option<String>,
        /// Maximum number of records to return.
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u16).range(1..=100))]
        limit: u16,
    },
    /// Rebuild disposable projections from immutable records.
    Rebuild {
        /// Initialized store directory.
        #[arg(long)]
        store: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub enum SchemaCommand {
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
