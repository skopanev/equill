use crate::kernel::error::Error;
use serde::{Deserialize, Serialize};
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
    /// Narrow append grants evaluated after the legacy store-wide writers.
    /// `*` matches any valid actor, namespace, or registered record type.
    #[serde(default)]
    pub write_grants: Vec<WriteGrant>,
    pub created_at_unix_ms: u128,
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
    validate_write_grants(&config)?;
    Ok(config)
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
mod tests {
    use super::{StoreConfig, WriteGrant, validate_write_grants};
    use serde_json::json;

    fn metadata() -> serde_json::Value {
        json!({
            "format_version": 1,
            "root_owner": "owner",
            "namespaces": ["agent.memory"],
            "writers": [],
            "created_at_unix_ms": 1
        })
    }

    #[test]
    fn legacy_metadata_defaults_to_no_scoped_grants() {
        let config: StoreConfig = serde_json::from_value(metadata()).expect("legacy metadata");
        // Metadata written by a newer equill must still open in this one: the
        // grant shape is strict, the envelope around it deliberately is not.
        let mut forward = metadata();
        forward["future_field"] = json!("unknown to this version");

        assert!(config.write_grants.is_empty());
        serde_json::from_value::<StoreConfig>(forward).expect("unknown top-level field");
    }

    #[test]
    fn scoped_grants_reject_unknown_fields() {
        let mut value = metadata();
        value["write_grants"] = json!([{
            "actors": ["agent"],
            "namespace": "agent.memory",
            "types": ["agent.finding.v1"],
            "typo": true
        }]);

        assert!(serde_json::from_value::<StoreConfig>(value).is_err());
    }

    #[test]
    fn scoped_grants_reject_empty_or_control_dimensions() {
        let mut config: StoreConfig = serde_json::from_value(metadata()).expect("metadata");
        config.write_grants = vec![WriteGrant {
            actors: vec!["agent\n".into()],
            namespace: "agent.memory".into(),
            types: vec!["agent.finding.v1".into()],
        }];
        assert!(validate_write_grants(&config).is_err());

        config.write_grants[0].actors = vec!["agent".into()];
        config.write_grants[0].types.clear();
        assert!(validate_write_grants(&config).is_err());
    }
}
