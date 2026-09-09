use crate::kernel::{error::Error, path};
use serde::Serialize;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

pub(crate) fn directory(root: &Path, relative: &str) -> Result<(), Error> {
    let mut prefix = String::new();
    for part in relative.split('/') {
        let parent = if prefix.is_empty() {
            root.to_owned()
        } else {
            path::within(root, &prefix)?
        };
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(part);
        let directory = path::within(root, &prefix)?;
        if !directory.exists() {
            fs::create_dir(&directory)?;
            File::open(parent)?.sync_all()?;
        }
        if !directory.is_dir() {
            return Err(Error::Integrity(
                "operation directory is not a directory".into(),
            ));
        }
    }
    Ok(())
}

pub(crate) fn publish(root: &Path, relative: &str, value: &impl Serialize) -> Result<(), Error> {
    let (parent, _) = relative
        .rsplit_once('/')
        .ok_or_else(|| Error::Integrity("operation path".into()))?;
    directory(root, parent)?;
    let target = path::within(root, relative)?;
    let temporary = path::within(root, &format!("{parent}/.{}.tmp", uuid::Uuid::now_v7()))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        serde_json::to_writer(&mut file, value)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temporary, target)?;
        File::open(path::within(root, parent)?)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

pub(crate) fn remove(root: &Path, relative: &str) -> Result<(), Error> {
    let target = path::file_within(root, relative)?;
    fs::remove_file(&target)?;
    File::open(
        target
            .parent()
            .ok_or_else(|| Error::Integrity("operation parent".into()))?,
    )?
    .sync_all()?;
    Ok(())
}

pub(crate) fn is_temporary(name: &str) -> bool {
    name.strip_prefix('.')
        .and_then(|name| name.strip_suffix(".tmp"))
        .and_then(|name| uuid::Uuid::parse_str(name).ok())
        .is_some_and(|id| id.get_version() == Some(uuid::Version::SortRand))
}

pub(crate) fn clean_temporary(root: &Path, relative: &str) -> Result<(), Error> {
    let directory = path::within(root, relative)?;
    if directory.exists() {
        for entry in fs::read_dir(directory)? {
            let name = path::plain_name(&entry?.path())?;
            if is_temporary(&name) {
                remove(root, &format!("{relative}/{name}"))?;
            }
        }
    }
    Ok(())
}
