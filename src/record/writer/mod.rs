mod blocked;

use super::{AppendReport, RecordDraft, StoredRecord};
use crate::defense;
use crate::kernel::digest::sha256_hex;
use crate::kernel::error::Error;
use crate::kernel::identity;
use crate::kernel::lock::StoreLock;
use crate::kernel::store;
use crate::projection::ProjectionState;
use crate::schema;
use jiff::Timestamp;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use uuid::Uuid;

use super::receipt::{self, WriteReceipt, WriteStatus};
use blocked::{block_write, ensure_clean_tail, unreachable_report};

pub fn append_file(store_root: &Path, source: &Path, actor: &str) -> Result<AppendReport, Error> {
    let draft: RecordDraft = serde_json::from_slice(&fs::read(source)?)?;
    append(store_root, draft, actor)
}

/// One record, then a catch-up. A batch wants exactly one catch-up for the
/// whole set rather than one per line — loading the model forty times to index
/// forty records is the difference between a usable import and an unusable one
/// — so batch callers use `append_only` and drain once at the end.
pub fn append(store_root: &Path, draft: RecordDraft, actor: &str) -> Result<AppendReport, Error> {
    let (mut report, _record) = confirm(store_root, draft, actor)?;
    // Nothing but waking the worker. The text index used to be written here,
    // one transaction inside the call the user waits on, on the argument that a
    // record should be findable the instant it is written.
    //
    // That argument was about read semantics, and it has been answered on the
    // read side rather than paid for here: a search now reports whether the
    // index is current or behind, so a caller can tell "not there" from "not
    // there yet" without every write funding the distinction. What is left is a
    // signal to the worker, which does not wait for it.
    report.vector = crate::vector::after_commit(store_root, 1);
    Ok(report)
}

/// Re-read the authority from disk and check it again. Called while the writer
/// lock is held, immediately before the append.
///
/// The check at the top of `append_only` happens before any lock is taken, so a
/// handover landing in between would otherwise let an actor who has just lost
/// access write anyway — authorized against a store that no longer exists.
pub(crate) fn require_current_writer(
    store_root: &Path,
    actor: &str,
    namespace: &str,
    type_name: &str,
) -> Result<(), Error> {
    identity::require_type_writer(&store::load(store_root)?, actor, namespace, type_name)
}

pub fn append_only(
    store_root: &Path,
    draft: RecordDraft,
    actor: &str,
) -> Result<AppendReport, Error> {
    confirm(store_root, draft, actor).map(|(report, _)| report)
}

/// The confirmation itself, returning the record it wrote.
///
/// Callers that need to index or project the record afterwards already have it
/// here; making them read it back would put a ledger read into the path this
/// work exists to keep clear of one.
fn confirm(
    store_root: &Path,
    mut draft: RecordDraft,
    actor: &str,
) -> Result<(AppendReport, StoredRecord), Error> {
    let config = store::load(store_root)?;
    identity::require_type_writer(&config, actor, &draft.namespace, &draft.type_name)?;
    let recorded_at = Timestamp::now().to_string();
    let month = month(&recorded_at)?;
    let defense = defense::apply(store_root, &mut draft)?;
    if defense.blocked() {
        // A refused write has no record to hand back.
        return block_write(store_root, &draft, actor, &recorded_at, &month, defense)
            .map(unreachable_report);
    }
    let definition = schema::load(store_root, &draft.type_name)?;
    super::validation::validate(&draft, &config, &definition)?;

    let redacted = defense.redacted();
    let valid_at = draft
        .valid_at
        .clone()
        .unwrap_or_else(|| draft.observed_at.clone());
    let record = StoredRecord {
        id: Uuid::now_v7(),
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
    };
    let mut line = serde_json::to_vec(&record)?;
    let digest = sha256_hex(&line);
    line.push(b'\n');
    let ledger = format!("records/{month}.jsonl");
    let path = store_root.join(&ledger);
    let receipt = WriteReceipt {
        receipt_id: record.id,
        status: WriteStatus::Appended,
        record_id: Some(record.id),
        namespace: &record.namespace,
        type_name: &record.type_name,
        actor,
        recorded_at: &record.recorded_at,
        record_sha256: Some(&digest),
        // The receipt is written before the append lands, so it states the
        // intent of a durable write; a receipt only ever accompanies one that
        // committed. The record being written is not indexed yet, so the
        // projection is queued whenever there is one — no marker read needed to
        // establish something already known.
        durable: true,
        projection: crate::vector::projection_after_write(store_root),
        defense_findings: &defense.findings,
    };

    let _lock = StoreLock::exclusive(store_root)?;
    // Finish anything a previous write left unfinished, before this one is
    // authorized. A store holding a durable record whose receipt never
    // committed must not accept another write on top of it: the second write
    // would succeed, the first would stay unaccounted for, and nothing after
    // that could tell the two apart.
    receipt::resolve_pending(store_root)?;
    // Authority is re-read and re-checked here, inside the lock, immediately
    // before the append. The check above happened before any lock was held, so
    // a handover that landed in between would otherwise let an actor who has
    // just lost access write anyway — authorized against a store that no longer
    // exists.
    require_current_writer(store_root, actor, &record.namespace, &record.type_name)?;
    // Lifecycle validation consults compact state, not the ledger. The state
    // carries only what the rules read — type, namespace, supersedes, key — and
    // is refused unless its watermark still describes the ledger, so a store it
    // no longer matches is rebuilt from truth rather than trusted.
    //
    // Rebuilding is the exception, not the path: it happens once, on a store
    // that has never had state or whose ledger moved beneath it.
    let mut lifecycle = match super::lifecycle::load_state(store_root)? {
        Some(state) => state,
        None => super::lifecycle::rebuild_state(store_root)?,
    };
    // The type registry, not the ledger: a handful of small files naming every
    // type, which is what tells us which linear types could claim this record
    // as one of their heads.
    let claiming = super::lifecycle::registered_types(store_root)?;
    let target_definition = record
        .supersedes
        .and_then(|id| lifecycle.entries.get(&id))
        .map(|entry| schema::load(store_root, &entry.type_name))
        .transpose()?;
    super::lifecycle::validate_append_against(
        &lifecycle,
        &record,
        &definition,
        target_definition.as_ref(),
        &claiming,
    )?;
    ensure_clean_tail(&path)?;
    let staged = receipt::stage(store_root, &month, &receipt)?;
    let receipt_path = staged.relative().to_owned();
    let handle = staged.handle().to_owned();
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(&line)?;
    file.sync_data()?;
    lifecycle.record(&record, super::lifecycle::keys_of(&record, &claiming));
    staged.commit().map_err(|error| {
        // The record is in the ledger and stays there. The error names the
        // coordinate it was written under and where the unfinished receipt is,
        // so the transaction can be finished rather than guessed at — and the
        // next write against this store finishes it before doing anything else.
        Error::PostCommit(format!(
            "record {} is durable but its receipt is not committed: {error}; \
             recovery handle: {handle}",
            record.id
        ))
    })?;
    // Everything from here is rebuildable, and everything from here is after
    // the point of no return: the record is durable and its receipt is
    // committed, so the write HAS succeeded. Failing it now would report a
    // completed write as a failed one, which is the worse lie — a caller would
    // retry and the store would hold the record twice.
    //
    // The state follows the ledger, never leads it: if this fails, the next
    // write finds a watermark that no longer matches and rebuilds, which costs
    // one scan rather than a wrong answer.
    let _ = super::lifecycle::save_state(store_root, &mut lifecycle);
    // Where the ledger now stands, said out loud so that neither the text nor
    // the vector side has to look at the ledger to know what "current" means.
    // Both numbers are already in hand: the state just counted the records and
    // just stamped the byte position.
    // Likewise: an unpublished target reads as unknown freshness, which is the
    // honest answer while it is missing, and the next write publishes it again.
    let _ = crate::projection::publish_target(
        store_root,
        lifecycle.entries.len(),
        lifecycle.watermark.bytes,
    );

    // Confirmation ends here. The ledger holds the record and its receipt is
    // committed, which is the whole of what "durable" claims — and everything
    // below is rebuildable from exactly that.
    //
    // The text index used to be written here. It is constant-cost, which is why
    // it went unnoticed, but constant cost is a reason it was not urgent rather
    // than a reason it belonged: a caller waiting on a projection is waiting on
    // something the ledger can reconstruct.
    //
    // The similarity advisory used to be computed here too, and it read the
    // entire ledger to do it. A hint worth having is not worth making every
    // write pay for the whole store's history.
    Ok((
        AppendReport {
            ok: true,
            durable: true,
            vector: crate::vector::DrainReport::default(),
            similar: Vec::new(),
            id: record.id,
            sha256: digest.clone(),
            ledger: ledger.clone(),
            receipt: receipt_path,
            redacted,
            projection: ProjectionState::Queued,
        },
        record,
    ))
}

fn month(timestamp: &str) -> Result<String, Error> {
    timestamp
        .get(..7)
        .map(str::to_owned)
        .ok_or_else(|| Error::InvalidRecord("system clock is out of range".into()))
}
