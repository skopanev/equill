//! Which store, if any, a command opens — the question the automatic vector
//! catch-up asks before deciding whether to nudge anything along.
use super::args::{RegistryCommand, SchemaCommand};
use super::authority::{GrantCommand, OwnerCommand};
use super::commands::Command;

impl Command {
    /// The store this command opens, when it should also nudge a lagging vector
    /// index along.
    ///
    /// The exception set, and why each one is in it:
    ///
    /// - `vector drain` IS the worker; nudging itself would start a second copy.
    /// - `vector configure`/`disable`/`rebuild`/`sync` own their own recovery
    ///   path. Racing an automatic worker against a rebuild that is about to
    ///   replace the collection is how a maintenance command gets undermined by
    ///   the very mechanism meant to help it.
    /// - `init` has no store to speak of yet.
    /// - `record` and `import` hand off for themselves after committing, so
    ///   nudging them first is pure duplicated work on the hottest path there
    ///   is: two marker reads before every write, to reach a conclusion the
    ///   write is about to reach anyway.
    ///
    /// Everything else that opens a store is included — `status` and `doctor`
    /// among them. A health check that reports a lagging index while declining
    /// to let it catch up is reporting a problem it could have ended.
    pub fn store_to_resume(&self) -> Option<&std::path::Path> {
        match self {
            Self::Compact { store, .. }
            | Self::Context { store, .. }
            | Self::Search { store, .. }
            | Self::Get { store, .. }
            | Self::Revoke { store, .. }
            | Self::Mcp { store, .. }
            | Self::Rebuild { store, .. } => Some(store),
            Self::Schema { command } => command.store(),
            Self::Profile { command } | Self::Selector { command } => command.store(),
            Self::Vector { command } => command.store_to_resume(),
            Self::Owner { command } => command.store(),
            Self::Grant { command } => command.store(),
            Self::Doctor { store, .. } => store.as_deref(),
            Self::Status { store } => store.as_deref(),
            Self::Init { .. } | Self::Record { .. } | Self::Import { .. } => None,
        }
    }
}

impl SchemaCommand {
    fn store(&self) -> Option<&std::path::Path> {
        match self {
            Self::List { store } | Self::Show { store, .. } | Self::Register { store, .. } => {
                Some(store)
            }
        }
    }
}

impl RegistryCommand {
    fn store(&self) -> Option<&std::path::Path> {
        match self {
            Self::Register { store, .. } => Some(store),
        }
    }
}

impl OwnerCommand {
    fn store(&self) -> Option<&std::path::Path> {
        match self {
            Self::Show { store } | Self::Transfer { store, .. } => Some(store),
        }
    }
}

impl GrantCommand {
    fn store(&self) -> Option<&std::path::Path> {
        match self {
            Self::List { store } | Self::Add { store, .. } | Self::Revoke { store, .. } => {
                Some(store)
            }
        }
    }
}
