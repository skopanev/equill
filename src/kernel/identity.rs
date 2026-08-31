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
    valid(actor) && allowed.iter().any(|item| item == "*" || item == actor)
}

/// Appending records is allowed for the root owner and for any actor the store
/// lists as a writer. Governance — schemas, selectors, profiles, compaction —
/// stays with the root owner.
pub fn require_writer(config: &StoreConfig, actor: &str) -> Result<(), Error> {
    if valid(actor) && (actor == config.root_owner || permits(&config.writers, actor)) {
        Ok(())
    } else {
        Err(Error::PermissionDenied)
    }
}

/// Require append access for one namespace and record type. Root ownership and
/// legacy `writers` remain store-wide; `write_grants` add least-privilege access.
pub fn require_type_writer(
    config: &StoreConfig,
    actor: &str,
    namespace: &str,
    type_name: &str,
) -> Result<(), Error> {
    let scoped = config.write_grants.iter().any(|grant| {
        permits(&grant.actors, actor)
            && matches(&grant.namespace, namespace)
            && grant.types.iter().any(|item| matches(item, type_name))
    });
    if valid(actor) && (actor == config.root_owner || permits(&config.writers, actor) || scoped) {
        Ok(())
    } else {
        Err(Error::PermissionDenied)
    }
}

fn matches(expected: &str, actual: &str) -> bool {
    expected == "*" || expected == actual
}

pub fn valid(identity: &str) -> bool {
    !identity.trim().is_empty() && !identity.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::{permits, require_type_writer};
    use crate::kernel::store::{StoreConfig, WriteGrant};

    fn config() -> StoreConfig {
        StoreConfig {
            default_context_profile: None,
            format_version: 1,
            root_owner: "owner".into(),
            namespaces: vec!["agent.memory".into()],
            writers: vec!["legacy".into()],
            write_grants: vec![WriteGrant {
                actors: vec!["finding-agent".into()],
                namespace: "agent.memory".into(),
                types: vec!["agent.finding.v1".into()],
            }],
            created_at_unix_ms: 1,
            extra: Default::default(),
        }
    }

    #[test]
    fn scoped_writer_cannot_append_another_type() {
        let config = config();

        require_type_writer(&config, "finding-agent", "agent.memory", "agent.finding.v1")
            .expect("matching grant");
        assert!(
            require_type_writer(&config, "finding-agent", "agent.memory", "agent.lesson.v1")
                .is_err()
        );
    }

    #[test]
    fn root_legacy_writer_and_wildcards_keep_expected_access() {
        let mut config = config();
        config.write_grants.push(WriteGrant {
            actors: vec!["*".into()],
            namespace: "*".into(),
            types: vec!["audit.event.v1".into()],
        });
        config.write_grants.push(WriteGrant {
            actors: vec!["type-agent".into()],
            namespace: "agent.memory".into(),
            types: vec!["*".into()],
        });

        for actor in ["owner", "legacy"] {
            require_type_writer(&config, actor, "other.space", "any.type.v1")
                .expect("broad writer");
        }
        require_type_writer(&config, "other-agent", "other.space", "audit.event.v1")
            .expect("wildcard grant");
        require_type_writer(&config, "type-agent", "agent.memory", "any.type.v1")
            .expect("wildcard type");
        assert!(!permits(&["*".into()], "\n"));
    }
}
