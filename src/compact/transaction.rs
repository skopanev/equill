use super::model::Plan;
use super::receipt;
use crate::command::doctor;
use crate::kernel::error::Error;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub struct SourceStage {
    original: PathBuf,
    temporary: PathBuf,
    backup: PathBuf,
    changed: bool,
    committed: bool,
}

impl SourceStage {
    pub fn import_path(&self) -> &Path {
        if self.changed {
            &self.temporary
        } else {
            &self.original
        }
    }
}

pub fn stage_sources(plan: &Plan, transaction: &str) -> Result<Vec<SourceStage>, Error> {
    let mut stages = Vec::with_capacity(plan.inputs.len());
    for input in &plan.inputs {
        match stage_one(input, transaction) {
            Ok(stage) => stages.push(stage),
            Err(error) => {
                cleanup_sources(&stages);
                return Err(error);
            }
        }
    }
    Ok(stages)
}

fn stage_one(input: &super::model::PlannedInput, transaction: &str) -> Result<SourceStage, Error> {
    let temporary = sibling(&input.source, "stage", transaction)?;
    let backup = sibling(&input.source, "backup", transaction)?;
    let changed = input.before != input.after;
    if changed {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&input.after)?;
        file.sync_all()?;
    }
    Ok(SourceStage {
        original: input.source.clone(),
        temporary,
        backup,
        changed,
        committed: false,
    })
}

pub struct Swap {
    current: PathBuf,
    backup: PathBuf,
}

pub fn commit(
    store: &Path,
    shadow: &Path,
    transaction: &str,
    plan: &Plan,
    sources: &mut [SourceStage],
) -> Result<Vec<Swap>, Error> {
    for index in 0..sources.len() {
        if !sources[index].changed {
            continue;
        }
        if let Err(error) = commit_source(&plan.inputs[index], &mut sources[index]) {
            rollback_sources(sources);
            return Err(error);
        }
    }
    let mut swaps = Vec::new();
    for relative in [
        "records",
        "projections",
        "receipts/writes",
        "receipts/imports",
    ] {
        let current = store.join(relative);
        let incoming = shadow.join(relative);
        let backup = match sibling(&current, "backup", transaction) {
            Ok(path) => path,
            Err(error) => {
                rollback_swaps(&swaps);
                rollback_sources(sources);
                return Err(error);
            }
        };
        if let Err(error) = swap(&current, &incoming, &backup) {
            rollback_swaps(&swaps);
            rollback_sources(sources);
            return Err(error);
        }
        swaps.push(Swap { current, backup });
    }
    Ok(swaps)
}

fn commit_source(input: &super::model::PlannedInput, stage: &mut SourceStage) -> Result<(), Error> {
    if fs::read(&stage.original)? != input.before {
        return Err(Error::Compact(format!(
            "input changed while compacting: {}",
            input.declared.display()
        )));
    }
    fs::rename(&stage.original, &stage.backup)?;
    if let Err(error) = fs::rename(&stage.temporary, &stage.original) {
        let _ = fs::rename(&stage.backup, &stage.original);
        return Err(error.into());
    }
    stage.committed = true;
    Ok(())
}

pub fn finish(
    store: &Path,
    receipt: receipt::StagedReceipt,
    swaps: Vec<Swap>,
    sources: &mut [SourceStage],
) -> Result<(), Error> {
    let result = doctor::report(Some(store), true, false).and_then(|report| {
        if report.ok {
            receipt.commit()
        } else {
            Err(Error::Compact(
                "committed store failed doctor --full".into(),
            ))
        }
    });
    if let Err(error) = result {
        rollback_swaps(&swaps);
        rollback_sources(sources);
        return Err(error);
    }
    for swap in swaps {
        cleanup_tree(&swap.backup);
    }
    for source in sources.iter_mut().filter(|source| source.committed) {
        let _ = fs::remove_file(&source.backup);
        source.committed = false;
    }
    Ok(())
}

fn swap(current: &Path, incoming: &Path, backup: &Path) -> Result<(), Error> {
    fs::rename(current, backup)?;
    if let Err(error) = fs::rename(incoming, current) {
        let _ = fs::rename(backup, current);
        return Err(error.into());
    }
    Ok(())
}

fn rollback_swaps(swaps: &[Swap]) {
    for swap in swaps.iter().rev() {
        cleanup_tree(&swap.current);
        let _ = fs::rename(&swap.backup, &swap.current);
    }
}

fn rollback_sources(sources: &mut [SourceStage]) {
    for source in sources.iter_mut().rev().filter(|source| source.committed) {
        let _ = fs::remove_file(&source.original);
        let _ = fs::rename(&source.backup, &source.original);
        source.committed = false;
    }
}

pub fn cleanup_sources(sources: &[SourceStage]) {
    for source in sources {
        if !source.committed {
            let _ = fs::remove_file(&source.temporary);
        }
    }
}

pub fn sibling(path: &Path, kind: &str, transaction: &str) -> Result<PathBuf, Error> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Compact("path has no parent".into()))?;
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| Error::Compact("path is not UTF-8".into()))?;
    Ok(parent.join(format!(".{name}.equill-compact-{kind}-{transaction}")))
}

pub fn cleanup_tree(path: &Path) {
    if path.is_dir() {
        let _ = fs::remove_dir_all(path);
    } else if path.exists() {
        let _ = fs::remove_file(path);
    }
}
