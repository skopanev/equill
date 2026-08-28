use crate::kernel::error::Error;
use crate::kernel::store::StoreConfig;

const ACTOR_ENV: &str = "EQUILL_ACTOR";

pub fn actor_from_env() -> Result<String, Error> {
    let actor = std::env::var(ACTOR_ENV).map_err(|_| Error::MissingActor)?;
    if !valid(&actor) {
        return Err(Error::InvalidActor);
    }
    Ok(actor)
}

pub fn require_root(config: &StoreConfig, actor: &str) -> Result<(), Error> {
    if actor == config.root_owner {
        Ok(())
    } else {
        Err(Error::PermissionDenied)
    }
}

/// `*` in an allow-list means "any valid actor". Used for read access on a
/// profile and for write access on a store that is shared between agents.
pub fn permits(allowed: &[String], actor: &str) -> bool {
    allowed.iter().any(|item| item == "*" || item == actor)
}

/// Appending records is allowed for the root owner and for any actor the store
/// lists as a writer. Governance — schemas, selectors, profiles, compaction —
/// stays with the root owner.
pub fn require_writer(config: &StoreConfig, actor: &str) -> Result<(), Error> {
    if actor == config.root_owner || permits(&config.writers, actor) {
        Ok(())
    } else {
        Err(Error::PermissionDenied)
    }
}

pub fn valid(identity: &str) -> bool {
    !identity.trim().is_empty() && !identity.chars().any(char::is_control)
}
