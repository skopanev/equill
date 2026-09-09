use super::failure;
use crate::kernel::error::Error;
use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

struct Staging(PathBuf);
impl Drop for Staging {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) fn bundle(
    store: &Path,
    output: &Path,
    files: &[(String, Vec<u8>)],
) -> Result<(), Error> {
    let source = store
        .canonicalize()
        .map_err(|_| failure("source store is unavailable"))?;
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = parent
        .canonicalize()
        .map_err(|_| failure("output parent must exist"))?;
    let name = output
        .file_name()
        .ok_or_else(|| failure("output must name a new directory"))?;
    let destination = parent.join(name);
    if destination.starts_with(&source) {
        return Err(failure("output must be outside the source store"));
    }
    if destination.symlink_metadata().is_ok() {
        return Err(failure("output already exists"));
    }
    let staging = parent.join(format!(".equill-schema-export-{}", uuid::Uuid::now_v7()));
    fs::create_dir(&staging).map_err(|_| failure("output staging failed"))?;
    let staging = Staging(staging);
    for (name, bytes) in files {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(staging.0.join(name))
            .map_err(|_| failure("output file staging failed"))?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| failure("output file write failed"))?;
    }
    File::open(&staging.0)
        .and_then(|file| file.sync_all())
        .map_err(|_| failure("output staging sync failed"))?;
    publish_new(&staging.0, &destination)?;
    File::open(parent)
        .and_then(|file| file.sync_all())
        .map_err(|_| failure("output directory sync failed"))?;
    Ok(())
}

/// Atomic no-replace publication: a racing creator must not be overwritten.
fn publish_new(source: &Path, destination: &Path) -> Result<(), Error> {
    let source =
        CString::new(source.as_os_str().as_bytes()).map_err(|_| failure("invalid output path"))?;
    let destination = CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| failure("invalid output path"))?;
    // SAFETY: both arguments are live NUL-terminated path strings. These calls
    // atomically rename only when the destination does not already exist.
    #[cfg(target_os = "macos")]
    let result =
        unsafe { libc::renamex_np(source.as_ptr(), destination.as_ptr(), libc::RENAME_EXCL) };
    #[cfg(target_os = "linux")]
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let result = -1;
    if result != 0 {
        return Err(failure(
            "output publication failed; destination must not exist",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_racing_empty_destination_is_not_replaced() {
        let root =
            std::env::temp_dir().join(format!("equill-export-race-{}", uuid::Uuid::now_v7()));
        fs::create_dir(&root).unwrap();
        let source = root.join("staged");
        let destination = root.join("destination");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("manifest.json"), b"synthetic").unwrap();
        fs::create_dir(&destination).unwrap();
        assert!(publish_new(&source, &destination).is_err());
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 0);
        assert!(source.join("manifest.json").is_file());
        fs::remove_dir_all(root).unwrap();
    }
}
