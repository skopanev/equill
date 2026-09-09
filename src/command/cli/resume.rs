//! Which store, if any, a command opens — the question the automatic vector
//! catch-up asks before deciding whether to nudge anything along.
use super::args::{RegistryCommand, SchemaCommand};
use super::authority::{GrantCommand, OwnerCommand, ReaderCommand};
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
    /// Read-only invocations stay passive: starting maintenance immediately
    /// before capturing their answer makes their own worker race the read.
    /// Writes retain their after-commit handoff; explicit vector maintenance
    /// remains available when no further writes arrive.
    pub fn store_to_resume(&self) -> Option<&std::path::Path> {
        match self {
            Self::Compact { store, .. }
            | Self::Revoke { store, .. }
            | Self::Mcp { store, .. }
            | Self::Rebuild { store, .. } => Some(store),
            Self::Schema { command } => command.store(),
            Self::Profile { command } | Self::Selector { command } => command.store(),
            Self::Vector { command } => command.store_to_resume(),
            Self::Owner { command } => command.store(),
            Self::Grant { command } => command.store(),
            Self::Reader { command } => command.store(),
            Self::Context { .. }
            | Self::Search { .. }
            | Self::Get { .. }
            | Self::Doctor { .. }
            | Self::Status { .. }
            | Self::Audit { .. }
            | Self::Init { .. }
            | Self::Record(_)
            | Self::Import { .. } => None,
        }
    }
}

impl SchemaCommand {
    fn store(&self) -> Option<&std::path::Path> {
        match self {
            Self::Export { .. } | Self::List { .. } | Self::Show { .. } => None,
            Self::Register { store, .. } => Some(store),
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

impl ReaderCommand {
    pub fn store(&self) -> Option<&std::path::Path> {
        match self {
            Self::List { store } | Self::Add { store, .. } | Self::Revoke { store, .. } => {
                Some(store)
            }
        }
    }
}

/// Whether the caller is an actor this store holds to reading.
///
/// The actor comes from the environment here and from the session there, so the
/// question is asked in one place and answered by the kernel: two surfaces that
/// decided this separately would eventually decide it differently.
pub fn held_to_reading(store: &std::path::Path) -> bool {
    crate::kernel::identity::actor_from_env()
        .map(|actor| crate::kernel::store::holds_to_reading(store, &actor))
        .unwrap_or(false)
}
