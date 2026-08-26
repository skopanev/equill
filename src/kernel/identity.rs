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

pub fn valid(identity: &str) -> bool {
    !identity.trim().is_empty() && !identity.chars().any(char::is_control)
}
