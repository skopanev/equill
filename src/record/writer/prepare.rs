use super::authority::require_draft_writer;
use crate::defense::{self, DefenseResult};
use crate::kernel::{error::Error, store};
use crate::record::{RecordDraft, StoredRecord};
use crate::schema::{self, TypeDefinition};
use std::path::Path;
use uuid::Uuid;

pub(super) struct Prepared {
    pub record: StoredRecord,
    pub definition: TypeDefinition,
    pub defense: DefenseResult,
}

pub(super) enum Preparation {
    Allowed(Box<Prepared>),
    Blocked(Box<(RecordDraft, DefenseResult)>),
}

/// Shared by a single append and an atomic import, before either mutates truth.
pub(super) fn prepare(
    root: &Path,
    mut draft: RecordDraft,
    actor: &str,
    revoking: Option<&StoredRecord>,
    id: Uuid,
    recorded_at: String,
) -> Result<Preparation, Error> {
    let config = store::load(root)?;
    let defense = defense::apply(root, &mut draft)?;
    if defense.blocked() {
        return Ok(Preparation::Blocked(Box::new((draft, defense))));
    }
    let definition = schema::load(root, &draft.type_name)?;
    crate::record::validation::validate(&draft, &config, &definition)?;
    require_draft_writer(&config, actor, &draft)?;
    // Only a trusted retraction of unchanged stored content is exempt.
    if !revoking.is_some_and(|target| target.payload == draft.payload) {
        crate::record::word_limit::check(
            &crate::retrieval::word_limits(root)?,
            &draft.type_name,
            &draft.payload,
        )?;
    }
    let valid_at = draft.valid_at.unwrap_or_else(|| draft.observed_at.clone());
    Ok(Preparation::Allowed(Box::new(Prepared {
        definition,
        defense,
        record: StoredRecord {
            id,
            namespace: draft.namespace,
            type_name: draft.type_name,
            actor: actor.to_owned(),
            recorded_at,
            observed_at: draft.observed_at,
            valid_at,
            payload: draft.payload,
            evidence: draft.evidence,
            tags: draft.tags,
            supersedes: draft.supersedes,
        },
    })))
}
