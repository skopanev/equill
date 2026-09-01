use crate::kernel::error::Error;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize, Serialize)]
pub struct StoreConfig {
    pub format_version: u64,
    pub root_owner: String,
    pub namespaces: Vec<String>,
    /// Actors allowed to append records besides the root owner. Empty keeps the
    /// store owner-only; `["*"]` opens it to every agent on the machine.
    #[serde(default)]
    pub writers: Vec<String>,
    /// Actors that may read and never append, whatever else would allow them.
    ///
    /// A refusal rather than an absence, and it is needed because every other
    /// rule here grants: `writers` and `write_grants` say who may append, and
    /// `*` in either says everyone. A store that has opened itself with a
    /// wildcard cannot take one actor back out — removing a name from a list
    /// that does not contain names does nothing, and revoking the wildcard
    /// would take access from everybody at once.
    ///
    /// Exact names only. `*` is deliberately not honoured: a wildcard here
    /// would lock the store against every actor including its owner, which is
    /// a state no command could undo.
    ///
    /// This is a convention between cooperating agents, not a wall. An actor
    /// is whatever `EQUILL_ACTOR` says it is, so anyone who can run the binary
    /// can run it under a name this list does not mention. What the list buys
    /// is that an agent which means to read does not write by accident, and
    /// that the intent is recorded where both sides can see it. What keeps a
    /// store safe from somebody who does not mean to cooperate is control over
    /// who runs the binary, which lives outside Equill.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read_only: Vec<String>,
    /// Narrow append grants evaluated after the legacy store-wide writers.
    /// `*` matches any valid actor, namespace, or registered record type.
    #[serde(default)]
    pub write_grants: Vec<WriteGrant>,
    pub created_at_unix_ms: u128,
    /// Top-level fields this build does not know about.
    ///
    /// The profile a context call uses when the caller does not name one.
    ///
    /// The store knows what it is for; a caller should not have to repeat that
    /// back to it on every question. Absent means there is no default and the
    /// caller must name a profile, which is what every store did before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_context_profile: Option<String>,
    /// A store written by a newer Equill may carry keys this one has never heard
    /// of. Reading them is not enough: anything that rewrites the metadata has to
    /// write them back, or an ordinary grant silently deletes state the newer
    /// build depends on. A sorted map keeps that round-trip byte-deterministic.
    #[serde(flatten, default)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WriteGrant {
    pub actors: Vec<String>,
    pub namespace: String,
    pub types: Vec<String>,
}

pub fn load(root: &Path) -> Result<StoreConfig, Error> {
    let path = root.join("store.json");
    if !path.is_file() {
        return Err(Error::NotInitialized(root.to_path_buf()));
    }
    let config = serde_json::from_slice(&fs::read(path)?)?;
    validate(&config)?;
    Ok(config)
}

/// Every rule a store's metadata has to satisfy to be loadable.
///
/// Reading is not the only time this matters: anything that writes metadata has
/// to run it too, before serializing. Otherwise a write can produce a file that
/// this very function will refuse the next time anyone opens the store — a
/// change that succeeds and then bricks it.
pub fn validate(config: &StoreConfig) -> Result<(), Error> {
    if !crate::kernel::identity::valid(&config.root_owner) {
        return Err(Error::InvalidOwner);
    }
    if config.namespaces.is_empty() {
        return Err(Error::InvalidNamespace);
    }
    if config.writers.iter().any(|writer| !valid_match(writer)) {
        return Err(Error::InvalidActor);
    }
    validate_read_only(config)?;
    validate_write_grants(config)
}

/// The refusal list has to mean what it says the moment the store is opened.
///
/// The command that writes it already refuses these, but a config can be
/// edited by hand, and a list that names `*` or the owner would be a written
/// policy the store cannot enforce or cannot undo. Refusing at load makes the
/// store say so instead of behaving as though the entry were not there.
fn validate_read_only(config: &StoreConfig) -> Result<(), Error> {
    let mut seen = std::collections::BTreeSet::new();
    for actor in &config.read_only {
        // Each refusal names `read_only` and the name it objected to. The
        // generic actor error reads "EQUILL_ACTOR is not a stable identity",
        // which points at the environment of whoever is running the command —
        // and the fault is in the store's own file, which they may not have
        // written.
        //
        // Exact names only: `*` here would hold every actor including the
        // owner, and nothing could then lift it.
        if actor == "*" {
            return Err(Error::Governance(
                "read_only in store.json may not name `*`: it would hold every \
                 actor including the owner, and nothing could lift it"
                    .to_owned(),
            ));
        }
        if !crate::kernel::identity::valid(actor) {
            return Err(Error::Governance(format!(
                "read_only in store.json names {actor:?}, which is not a stable identity"
            )));
        }
        // The owner governs, and governance is what lifts a hold. Holding the
        // owner leaves a store nobody can recover.
        if actor == &config.root_owner {
            return Err(Error::Governance(format!(
                "read_only in store.json names the owner {actor}: the owner \
                 governs, and governance is what lifts a hold"
            )));
        }
        // A name twice is a list that disagrees with itself about how many
        // actors it holds, and the report that counts them would say two.
        if !seen.insert(actor) {
            return Err(Error::Governance(format!(
                "read_only in store.json names {actor} twice"
            )));
        }
    }
    Ok(())
}

/// Whether this store holds an actor to reading.
///
/// Best effort by design: a store that cannot be read here is one the caller is
/// about to fail on anyway, and answering "not held" leaves that failure to say
/// so properly rather than replacing it with this one.
pub fn holds_to_reading(root: &Path, actor: &str) -> bool {
    load(root)
        .map(|config| config.read_only.iter().any(|item| item == actor))
        .unwrap_or(false)
}

fn validate_write_grants(config: &StoreConfig) -> Result<(), Error> {
    for grant in &config.write_grants {
        if grant.actors.is_empty() || grant.actors.iter().any(|actor| !valid_match(actor)) {
            return Err(Error::InvalidActor);
        }
        if !valid_match(&grant.namespace)
            || (grant.namespace != "*"
                && !config
                    .namespaces
                    .iter()
                    .any(|namespace| namespace == &grant.namespace))
        {
            return Err(Error::InvalidNamespace);
        }
        if grant.types.is_empty() || grant.types.iter().any(|type_name| !valid_match(type_name)) {
            return Err(Error::InvalidType("store write grant".into()));
        }
    }
    Ok(())
}

fn valid_match(value: &str) -> bool {
    value == "*" || (!value.trim().is_empty() && !value.chars().any(char::is_control))
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
