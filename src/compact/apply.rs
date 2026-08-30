use super::model::{CompactReceipt, CompactReport, Plan};
use super::{receipt, transaction};
use crate::command::{doctor, init};
use crate::ingest::manifest::{self, ResolvedInput};
use crate::kernel::error::Error;
use crate::kernel::{lock::StoreLock, store};
use crate::{integrity, projection};
use std::fs;
use std::path::Path;

/// Called only from `compact::run`, which already holds the governance guard and
/// has already established that this actor governs.
pub fn execute(store_root: &Path, plan: Plan, actor: &str) -> Result<CompactReport, Error> {
    let config = store::load(store_root)?;
    let _lock = StoreLock::exclusive(store_root)?;
    let transaction_id = uuid::Uuid::now_v7().to_string();
    let mut sources = transaction::stage_sources(&plan, &transaction_id)?;
    let shadow = transaction::sibling(store_root, "shadow", &transaction_id)?;
    let prepared = prepare_shadow(store_root, &shadow, &plan, &sources, actor, &config);
    let (import_set_sha256, scan) = match prepared {
        Ok(value) => value,
        Err(error) => return abort(error, &shadow, &sources),
    };
    let removed = count_removals(&plan);
    let retained = count_retained(&plan);
    let compact_receipt = CompactReceipt {
        schema: "equill.compact-receipt.v1",
        manifest_sha256: plan.manifest_sha256.clone(),
        actor: actor.into(),
        timestamp: jiff::Timestamp::now().to_string(),
        inputs: plan
            .inputs
            .iter()
            .map(|input| input.public.clone())
            .collect(),
        removed,
        records: scan.records,
        projection_records: scan.projection_records,
        import_set_sha256,
        doctor_ok: true,
    };
    let staged_receipt = match receipt::stage(store_root, &compact_receipt) {
        Ok(receipt) => receipt,
        Err(error) => return abort(error, &shadow, &sources),
    };
    let receipt_path = staged_receipt.relative().to_owned();
    let swaps = transaction::commit(store_root, &shadow, &transaction_id, &plan, &mut sources);
    let result = swaps
        .and_then(|swaps| transaction::finish(store_root, staged_receipt, swaps, &mut sources));
    if let Err(error) = result {
        return abort(error, &shadow, &sources);
    }
    transaction::cleanup_tree(&shadow);
    Ok(CompactReport {
        ok: true,
        applied: true,
        manifest_sha256: plan.manifest_sha256,
        inputs: compact_receipt.inputs,
        removed,
        retained_with_reason: retained,
        receipt: Some(receipt_path),
    })
}

fn prepare_shadow(
    original: &Path,
    shadow: &Path,
    plan: &Plan,
    sources: &[transaction::SourceStage],
    actor: &str,
    config: &store::StoreConfig,
) -> Result<(String, integrity::FullScan), Error> {
    let namespace = config
        .namespaces
        .first()
        .ok_or_else(|| Error::Compact("store has no namespace".into()))?;
    init::create(shadow, &config.root_owner, namespace)?;
    fs::copy(original.join("store.json"), shadow.join("store.json"))?;
    copy_tree(&original.join("registry"), &shadow.join("registry"))?;
    let resolved = plan
        .inputs
        .iter()
        .zip(sources)
        .zip(&plan.entries)
        .map(|((input, stage), entry)| ResolvedInput {
            declared: input.declared.clone(),
            role: entry.role.clone(),
            source: stage.import_path().to_owned(),
        })
        .collect();
    let imported = manifest::import_resolved(shadow, &plan.manifest_bytes, resolved, actor)?;
    projection::rebuild(shadow)?;
    let report = doctor::report(Some(shadow), true, false)?;
    if !report.ok {
        return Err(Error::Compact("rebuilt store failed doctor --full".into()));
    }
    let scan = integrity::scan(shadow)?;
    let expected: usize = plan.inputs.iter().map(|input| input.records_after).sum();
    if scan.records != expected || scan.projection_records != expected {
        return Err(Error::Compact(format!(
            "rebuilt store has {} records; compacted inputs declare {expected}",
            scan.records
        )));
    }
    Ok((imported.set_sha256, scan))
}

fn copy_tree(source: &Path, target: &Path) -> Result<(), Error> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let destination = target.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &destination)?;
        } else {
            fs::copy(entry.path(), destination)?;
        }
    }
    Ok(())
}

fn count_removals(plan: &Plan) -> usize {
    plan.inputs
        .iter()
        .map(|input| input.public.removals.len())
        .sum()
}

fn count_retained(plan: &Plan) -> usize {
    plan.inputs
        .iter()
        .map(|input| input.public.retained.len())
        .sum()
}

fn abort<T>(error: Error, shadow: &Path, sources: &[transaction::SourceStage]) -> Result<T, Error> {
    transaction::cleanup_sources(sources);
    transaction::cleanup_tree(shadow);
    Err(error)
}
