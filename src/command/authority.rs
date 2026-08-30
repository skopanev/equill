//! The governance surfaces: who owns the store, and who may append to part of
//! it. Both read their actor from the environment like every other write.
use super::cli::{GrantCommand, OwnerCommand};
use crate::governance;
use crate::kernel::error::Error;
use crate::kernel::identity;

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
            comment,
        } => {
            let actor = identity::actor_from_env()?;
            let report = governance::grant(
                &store,
                &subject,
                &namespace,
                &types,
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
