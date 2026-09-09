//! Descriptor-relative storage: a renamed root or ancestor cannot redirect an
//! invocation's writes to the replacement path.
use super::failure;
use crate::kernel::error::Error;
use fs2::FileExt;
use std::ffi::{CStr, CString, OsString};
use std::fs::{self, File};
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path};

pub(in crate::audit) struct Directory(File);

impl Directory {
    pub(super) fn open(root: &Path) -> Result<Self, Error> {
        let absolute = if root.is_absolute() {
            root.to_owned()
        } else {
            std::env::current_dir()?.join(root)
        };
        // Resolve system aliases once, then walk real components without
        // following any replacement symlink. Missing components are created
        // relative to the already pinned parent, never through the input path.
        let existing = absolute
            .ancestors()
            .find(|p| p.exists())
            .ok_or_else(failure)?;
        let resolved = fs::canonicalize(existing)?
            .join(absolute.strip_prefix(existing).map_err(|_| failure())?);
        let mut directory = Self(File::open("/")?);
        for part in resolved.components() {
            if directory.exists("store.json")? {
                return Err(failure());
            }
            let Component::Normal(part) = part else {
                if part == Component::RootDir {
                    continue;
                }
                return Err(failure());
            };
            let name = CString::new(part.as_bytes()).map_err(|_| failure())?;
            let child = directory.open_directory(&name);
            let child = match child {
                Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                    let created = cvt(unsafe {
                        libc::mkdirat(directory.0.as_raw_fd(), name.as_ptr(), 0o700)
                    });
                    if !matches!(&created, Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::AlreadyExists)
                    {
                        created?;
                    }
                    directory.sync()?;
                    directory.open_directory(&name)?
                }
                other => other?,
            };
            directory = Self(child);
        }
        if directory.exists("store.json")? {
            return Err(failure());
        }
        Ok(directory)
    }

    fn open_directory(&self, name: &CStr) -> Result<File, Error> {
        let fd = cvt(unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        })?;
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    pub(super) fn file(&self, name: &str, create: bool) -> Result<File, Error> {
        self.open_file(name, if create { libc::O_CREAT } else { 0 })
    }

    pub(super) fn new_file(&self, name: &str) -> Result<File, Error> {
        self.open_file(name, libc::O_CREAT | libc::O_EXCL)
    }

    fn open_file(&self, name: &str, flags: i32) -> Result<File, Error> {
        let name = child_name(name)?;
        let open = |flags| {
            cvt(unsafe {
                libc::openat(
                    self.0.as_raw_fd(),
                    name.as_ptr(),
                    flags | libc::O_RDWR | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                    0o600,
                )
            })
        };
        // Concurrent O_CREAT on Darwin can return ENOENT after another opener
        // creates the leaf. Retry only an existing leaf; never relax O_EXCL.
        let fd = match open(flags) {
            Err(Error::Io(error))
                if flags == libc::O_CREAT && error.kind() == std::io::ErrorKind::NotFound =>
            {
                open(0)?
            }
            result => result?,
        };
        let file = unsafe { File::from_raw_fd(fd) };
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.nlink() != 1 {
            return Err(failure());
        }
        Ok(file)
    }

    pub(super) fn lock(&self) -> Result<File, Error> {
        let file = self.file("writer.lock", true)?;
        file.lock_exclusive()?;
        Ok(file)
    }

    pub(super) fn sync(&self) -> Result<(), Error> {
        Ok(self.0.sync_all()?)
    }

    pub(super) fn read(&self, name: &str) -> Result<Vec<u8>, Error> {
        let mut bytes = Vec::new();
        self.file(name, false)?
            .take(32_768)
            .read_to_end(&mut bytes)?;
        if bytes.len() == 32_768 {
            return Err(failure());
        }
        Ok(bytes)
    }

    pub(super) fn exists(&self, name: &str) -> Result<bool, Error> {
        let name = child_name(name)?;
        let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
        let result = unsafe {
            libc::fstatat(
                self.0.as_raw_fd(),
                name.as_ptr(),
                metadata.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if result == 0 {
            return Ok(true);
        }
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::NotFound {
            Ok(false)
        } else {
            Err(error.into())
        }
    }

    pub(super) fn rename(&self, from: &str, to: &str) -> Result<(), Error> {
        drop(self.file(from, false)?);
        if self.exists(to)? {
            drop(self.file(to, false)?);
        }
        let from = child_name(from)?;
        let to = child_name(to)?;
        cvt(unsafe {
            libc::renameat(
                self.0.as_raw_fd(),
                from.as_ptr(),
                self.0.as_raw_fd(),
                to.as_ptr(),
            )
        })?;
        Ok(())
    }

    pub(super) fn remove(&self, name: &str) -> Result<(), Error> {
        if self.exists(name)? {
            drop(self.file(name, false)?);
            let name = child_name(name)?;
            cvt(unsafe { libc::unlinkat(self.0.as_raw_fd(), name.as_ptr(), 0) })?;
        }
        Ok(())
    }

    pub(super) fn entries(&self) -> Result<Vec<OsString>, Error> {
        use std::os::fd::IntoRawFd;
        let fd = self.open_directory(c".")?.into_raw_fd();
        let pointer = unsafe { libc::fdopendir(fd) };
        if pointer.is_null() {
            let error = std::io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(error.into());
        }
        struct Listing(*mut libc::DIR);
        impl Drop for Listing {
            fn drop(&mut self) {
                unsafe { libc::closedir(self.0) };
            }
        }
        let listing = Listing(pointer);
        let mut names = Vec::new();
        loop {
            // readdir's null result means either EOF or errno. Clear errno
            // before each call so a genuine read failure cannot look empty.
            errno(0);
            let entry = unsafe { libc::readdir(listing.0) };
            if entry.is_null() {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() != Some(0) {
                    return Err(error.into());
                }
                break;
            }
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
            if name != b"." && name != b".." {
                names.push(OsString::from_vec(name.to_vec()));
            }
        }
        Ok(names)
    }
}

fn child_name(name: &str) -> Result<CString, Error> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        return Err(failure());
    }
    CString::new(name).map_err(|_| failure())
}

fn cvt(result: i32) -> Result<i32, Error> {
    if result < 0 {
        Err(std::io::Error::last_os_error().into())
    } else {
        Ok(result)
    }
}

fn errno(value: i32) {
    #[cfg(target_os = "macos")]
    unsafe {
        *libc::__error() = value
    };
    #[cfg(target_os = "linux")]
    unsafe {
        *libc::__errno_location() = value
    };
}
