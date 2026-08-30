//! The command list. Its argument vocabularies live beside it in `args`.
use super::args::*;
use super::authority::{GrantCommand, OwnerCommand};
use clap::Subcommand;
use std::path::PathBuf;

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
        /// Actor allowed to append records besides the owner. Repeatable; `*` opens the store.
        #[arg(long = "writer")]
        writers: Vec<String>,
    },
    /// Append one schema-validated immutable record.
    #[command(after_help = ACTOR_HELP)]
    Record {
        /// Initialized store directory.
        #[arg(long)]
        store: PathBuf,
        /// Record draft JSON file. Actor comes from EQUILL_ACTOR.
        #[arg(long)]
        input: PathBuf,
    },
    /// Import a legacy JSONL batch through the canonical writer.
    #[command(after_help = ACTOR_HELP)]
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
    #[command(after_help = ACTOR_HELP)]
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
    #[command(after_help = ACTOR_HELP)]
    Profile {
        #[command(subcommand)]
        command: RegistryCommand,
    },
    /// Manage canonical per-type selectors.
    #[command(after_help = ACTOR_HELP)]
    Selector {
        #[command(subcommand)]
        command: RegistryCommand,
    },
    /// Assemble deterministic bounded context from one explicit store.
    #[command(after_help = ACTOR_HELP)]
    Context {
        /// Initialized store directory.
        #[arg(long)]
        store: PathBuf,
        /// Registered context profile identifier.
        #[arg(long)]
        profile: String,
        /// Context request JSON file. Use it for scripted requests; for a
        /// one-off question `--query` and the coordinate flags are enough.
        #[arg(
            long,
            conflicts_with_all = [
                "query", "at", "coordinates", "project", "role", "phase", "harness", "tags",
                "kinds"
            ]
        )]
        request: Option<PathBuf>,
        /// Text to retrieve against, as an alternative to `--request`.
        #[arg(long)]
        query: Option<String>,
        /// Request coordinate as `key=value`. Repeatable.
        #[arg(long = "coordinate")]
        coordinates: Vec<String>,
        /// Project coordinate shorthand for `--coordinate project=VALUE`.
        #[arg(long, conflicts_with = "request")]
        project: Option<String>,
        /// Role coordinate shorthand for `--coordinate role=VALUE`.
        #[arg(long, conflicts_with = "request")]
        role: Option<String>,
        /// Phase coordinate shorthand for `--coordinate phase=VALUE`.
        #[arg(long, conflicts_with = "request")]
        phase: Option<String>,
        /// Harness coordinate shorthand for `--coordinate harness=VALUE`.
        #[arg(long, conflicts_with = "request")]
        harness: Option<String>,
        /// Request tag. Repeatable.
        #[arg(long = "tag")]
        tags: Vec<String>,
        /// Request kind. Repeatable.
        #[arg(long = "kind")]
        kinds: Vec<String>,
        /// Point in time the request is evaluated at. Defaults to now.
        #[arg(long)]
        at: Option<String>,
        /// Also return records a later one superseded, to read the chain.
        #[arg(long, conflicts_with = "request")]
        include_superseded: bool,
        #[command(flatten)]
        present: PresentationArgs,
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
        /// Text to match. Optional when a --where filter decides the result.
        #[arg(long)]
        query: Option<String>,
        /// Restrict matches to one namespace.
        #[arg(long)]
        namespace: Option<String>,
        /// Restrict matches to one registered type.
        #[arg(long = "type")]
        type_name: Option<String>,
        /// Maximum number of records to return.
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u16).range(1..=100))]
        limit: u16,
        /// Every match instead of one page (fts only; bounded by the scan cap).
        #[arg(long)]
        all: bool,
        /// Retrieval strategy. `hybrid` prefers semantics and falls back to text.
        #[arg(long, value_enum, default_value_t = StrategyArg::Fts)]
        strategy: StrategyArg,
        #[command(flatten)]
        present: PresentationArgs,
    },
    /// Manage the optional Qdrant vector projection.
    #[command(after_help = ACTOR_HELP)]
    Vector {
        #[command(subcommand)]
        command: VectorCommand,
    },
    /// Read one record by the id every result set prints.
    Get {
        /// Initialized store directory.
        #[arg(long)]
        store: PathBuf,
        /// Record identifier.
        #[arg(long)]
        id: String,
        /// Output shape for the record.
        #[arg(long, value_enum, default_value_t = FormatArg::Jsonl)]
        format: FormatArg,
        /// Print only these fields, in this order.
        #[arg(long, value_delimiter = ',')]
        fields: Vec<String>,
    },
    /// Withdraw a record by writing a tombstone that supersedes it.
    #[command(after_help = ACTOR_HELP)]
    Revoke {
        /// Initialized store directory.
        #[arg(long)]
        store: PathBuf,
        /// Identifier of the record to withdraw.
        #[arg(long)]
        id: String,
        /// Why it is being withdrawn. Stored as evidence, not as payload.
        #[arg(long)]
        comment: Option<String>,
    },
    /// Serve the local MCP adapter over stdio. No socket is opened.
    #[command(after_help = ACTOR_HELP)]
    Mcp {
        /// Initialized store directory.
        #[arg(long)]
        store: PathBuf,
    },
    /// Show or hand over root ownership of a store.
    Owner {
        #[command(subcommand)]
        command: OwnerCommand,
    },
    /// Manage scoped append grants.
    Grant {
        #[command(subcommand)]
        command: GrantCommand,
    },
    /// Rebuild disposable projections from immutable records.
    Rebuild {
        /// Initialized store directory.
        #[arg(long)]
        store: PathBuf,
    },
}
