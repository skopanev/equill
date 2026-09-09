use super::{Directory, failure, lock, transaction};
use crate::audit::Event;
use crate::kernel::error::Error;
use fs2::FileExt;
use std::fs::File;
use std::io::Write;
use std::path::Path;

pub(in crate::audit) struct Reservation {
    root: Directory,
    id: uuid::Uuid,
    _lease: File,
}

impl Reservation {
    pub(in crate::audit) fn begin(root: &Path, event: &Event) -> Result<Self, Error> {
        let held = lock(root)?;
        let root = &held.root;
        super::recover(root)?;
        let stage = name("intent-stage", event.id);
        let mut lease = root.new_file(&stage)?;
        lease.lock_exclusive()?;
        lease.write_all(&serde_json::to_vec(event)?)?;
        lease.sync_all()?;
        root.rename(&stage, &name("request", event.id))?;
        root.sync()?;
        Ok(Self {
            root: held.root,
            id: event.id,
            _lease: lease,
        })
    }

    pub(in crate::audit) fn checkpoint(&self, event: &Event) -> Result<(), Error> {
        if event.id != self.id || !event.valid() {
            return Err(failure());
        }
        let path = name("outcome-stage", self.id);
        let mut file = self.root.file(&path, true)?;
        file.set_len(0)?;
        file.write_all(&serde_json::to_vec(event)?)?;
        file.sync_all()?;
        self.root.rename(&path, &name("outcome", self.id))?;
        self.root.sync()?;
        let _lock = self.root.lock()?;
        drop(self.root.file("pending.tmp", true)?);
        drop(self.root.file(
            &format!("{}.jsonl", super::month(&event.observed_at)?),
            true,
        )?);
        Ok(())
    }

    pub(in crate::audit) fn finish(&self, event: &Event) -> Result<(), Error> {
        let _lock = self.root.lock()?;
        transaction::append(&self.root, event, Some(self.id))
    }
}

fn name(kind: &str, id: uuid::Uuid) -> String {
    format!("{kind}-{id}.json")
}

pub(super) fn remove(root: &Directory, id: uuid::Uuid) -> Result<(), Error> {
    for kind in ["outcome", "outcome-stage", "request"] {
        root.remove(&name(kind, id))?;
    }
    root.sync()?;
    Ok(())
}

pub(super) fn recover(root: &Directory) -> Result<(), Error> {
    for entry in root.entries()? {
        let Some(path) = entry.to_str() else {
            continue;
        };
        // Before this rename no domain operation was permitted to start.
        // An unpublished partial stage is disposable, never a completed event.
        if path
            .strip_prefix("intent-stage-")
            .and_then(|s| s.strip_suffix(".json"))
            .is_some_and(|s| s.parse::<uuid::Uuid>().is_ok())
        {
            let lease = root.file(path, false)?;
            match lease.try_lock_exclusive() {
                Ok(()) => {
                    root.remove(path)?;
                    root.sync()?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(error.into()),
            }
            continue;
        }
        let Some(id) = path
            .strip_prefix("request-")
            .and_then(|s| s.strip_suffix(".json"))
            .and_then(|s| s.parse::<uuid::Uuid>().ok())
        else {
            continue;
        };
        let lease = root.file(path, false)?;
        match lease.try_lock_exclusive() {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(error) => return Err(error.into()),
        }
        let outcome = name("outcome", id);
        let source = if root.exists(&outcome)? {
            &outcome
        } else {
            path
        };
        let mut event: Event = serde_json::from_slice(&root.read(source)?)?;
        if event.id != id || !event.valid() {
            return Err(failure());
        }
        event.outcome = "error".into();
        event.error_class = Some("interrupted".into());
        transaction::append(root, &event, Some(id))?;
    }
    Ok(())
}
