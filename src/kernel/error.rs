use std::fmt::{Display, Formatter};
use std::path::PathBuf;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Json(serde_json::Error),
    InvalidActor,
    InvalidOwner,
    InvalidNamespace,
    InvalidRecord(String),
    InvalidSchema(String),
    InvalidType(String),
    Compact(String),
    CommandRejected(String),
    Context(String),
    Filter(String),
    /// A governance call that is refused for a reason the caller has to read:
    /// the store is in a shape this operation cannot safely change.
    Governance(String),
    Integrity(String),
    Import(String),
    MemoryDefense(String),
    MissingActor,
    NotInitialized(PathBuf),
    PermissionDenied,
    /// The store lists this actor as read-only.
    ///
    /// Separate from `PermissionDenied` because the two are different answers
    /// to the caller: not being a writer is an absence that adding a grant
    /// fixes, while being read-only is a decision the store made about this
    /// actor and no grant overrides it. Telling them apart is the difference
    /// between "ask for access" and "you were not meant to have it".
    ReadOnlyActor(String),
    PostCommit(String),
    Projection(String),
    SchemaConflict(String),
    StoreExists(PathBuf),
    StoreMismatch,
}

impl Display for Error {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::Json(error) => write!(formatter, "invalid JSON: {error}"),
            Self::InvalidActor => write!(formatter, "EQUILL_ACTOR is not a stable identity"),
            Self::InvalidOwner => write!(formatter, "owner must be a non-empty stable identity"),
            Self::InvalidNamespace => write!(
                formatter,
                "namespace must contain dot-separated lowercase identifiers"
            ),
            Self::InvalidRecord(reason) => write!(formatter, "invalid record: {reason}"),
            Self::InvalidSchema(reason) => write!(formatter, "invalid schema: {reason}"),
            Self::InvalidType(reason) => write!(formatter, "invalid record type: {reason}"),
            Self::Compact(reason) => write!(formatter, "compaction failed: {reason}"),
            Self::CommandRejected(_) => write!(formatter, "command rejected one or more records"),
            Self::Context(reason) => write!(formatter, "context failed: {reason}"),
            Self::Filter(reason) => write!(formatter, "filter is not usable: {reason}"),
            Self::Integrity(reason) => write!(formatter, "integrity check failed: {reason}"),
            Self::Import(reason) => write!(formatter, "import failed: {reason}"),
            Self::MemoryDefense(reason) => write!(formatter, "memory defense failed: {reason}"),
            Self::MissingActor => write!(
                formatter,
                "EQUILL_ACTOR must be supplied by the calling orchestrator"
            ),
            Self::NotInitialized(path) => {
                write!(formatter, "not an Equill store: {}", path.display())
            }
            Self::SchemaConflict(name) => {
                write!(
                    formatter,
                    "record type already registered differently: {name}"
                )
            }
            Self::Governance(detail) => write!(formatter, "{detail}"),
            Self::PermissionDenied => write!(formatter, "actor is not allowed to write"),
            // A stable token first, so a caller can key on the refusal without
            // parsing prose, and the escalation path last, because the actor
            // reading this cannot lift the restriction itself.
            Self::ReadOnlyActor(actor) => write!(
                formatter,
                "PM_WRITE_DENIED: {actor} may read this store and may not change it. Escalate to GM."
            ),
            Self::PostCommit(reason) => write!(formatter, "post-commit failure: {reason}"),
            Self::Projection(reason) => write!(formatter, "projection failed: {reason}"),
            Self::StoreExists(path) => write!(
                formatter,
                "refusing to initialize existing non-store directory: {}",
                path.display()
            ),
            Self::StoreMismatch => write!(
                formatter,
                "store exists with different ownership or namespace"
            ),
        }
    }
}

impl Error {
    pub fn command_output(&self) -> Option<&str> {
        match self {
            Self::CommandRejected(output) => Some(output),
            _ => None,
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}
