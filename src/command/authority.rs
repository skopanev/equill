//! The governance surfaces: who owns the store, and who may append to part of
//! it. Both read their actor from the environment like every other write.
use super::cli::{GrantCommand, OwnerCommand, ReaderCommand};
use crate::governance;
use crate::kernel::error::Error;
use crate::kernel::identity;
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) fn owner(json: bool, command: OwnerCommand) -> Result<String, Error> {
    match command {
        OwnerCommand::Show { store } => {
            let report = governance::show(&store)?;
            super::output::render(json, &report, super::output::authority(&report))
        }
        OwnerCommand::Transfer { store, to, comment } => {
            let actor = identity::actor_from_env()?;
            let report = governance::transfer(&store, &to, comment.as_deref(), &actor)?;
            super::output::render(json, &report, super::output::owner(&report))
        }
    }
}

pub(crate) fn grant(json: bool, command: GrantCommand) -> Result<String, Error> {
    match command {
        GrantCommand::List { store } => {
            let report = governance::show(&store)?;
            super::output::render(json, &report, super::output::authority(&report))
        }
        GrantCommand::Add {
            store,
            actor: subject,
            namespace,
            types,
            payload_equals,
            comment,
        } => {
            let actor = identity::actor_from_env()?;
            let payload_equals = parse_payload_equals(&payload_equals)?;
            let report = governance::grant_with_payload_equals(
                &store,
                &subject,
                &namespace,
                &types,
                &payload_equals,
                comment.as_deref(),
                &actor,
            )?;
            super::output::render(json, &report, super::output::grant(&report))
        }
        GrantCommand::Revoke {
            store,
            actor: subject,
            comment,
        } => {
            let actor = identity::actor_from_env()?;
            let report = governance::revoke_grant(&store, &subject, comment.as_deref(), &actor)?;
            super::output::render(json, &report, super::output::grant(&report))
        }
    }
}

fn parse_payload_equals(entries: &[String]) -> Result<BTreeMap<String, Value>, Error> {
    let mut parsed = BTreeMap::new();
    for entry in entries {
        let (pointer, value) = entry.split_once('=').ok_or_else(|| {
            Error::Governance(format!(
                "payload constraint {entry:?} must be POINTER=VALUE"
            ))
        })?;
        if parsed
            .insert(pointer.to_owned(), Value::String(value.to_owned()))
            .is_some()
        {
            return Err(Error::Governance(format!(
                "payload constraint {pointer:?} was supplied twice"
            )));
        }
    }
    Ok(parsed)
}

pub(crate) fn reader(json: bool, command: ReaderCommand) -> Result<String, Error> {
    match command {
        ReaderCommand::List { store } => {
            let report = governance::show(&store)?;
            super::output::render(json, &report, super::output::authority(&report))
        }
        ReaderCommand::Add {
            store,
            actor: subject,
            comment,
        } => {
            let actor = identity::actor_from_env()?;
            let report = governance::deny_writes(&store, &subject, comment.as_deref(), &actor)?;
            super::output::render(json, &report, super::output::reader(&report))
        }
        ReaderCommand::Revoke {
            store,
            actor: subject,
            comment,
        } => {
            let actor = identity::actor_from_env()?;
            let report = governance::allow_writes(&store, &subject, comment.as_deref(), &actor)?;
            super::output::render(json, &report, super::output::reader(&report))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_payload_equals;

    #[test]
    fn payload_constraints_require_pointer_equals_value() {
        let error = parse_payload_equals(&["/project".into()]).expect_err("malformed");
        assert!(error.to_string().contains("POINTER=VALUE"));
    }

    #[test]
    fn payload_constraints_refuse_duplicate_pointers() {
        let error =
            parse_payload_equals(&["/project=project-a".into(), "/project=project-b".into()])
                .expect_err("duplicate");
        assert!(error.to_string().contains("supplied twice"));
    }
}
