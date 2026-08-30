use super::model::{ContextProfile, RegistryReport, Selector, VersionCoordinate};
use crate::kernel::digest::sha256_hex;
use crate::kernel::error::Error;
use crate::kernel::governance::RootGuard;
use crate::kernel::lock::StoreLock;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn register_profile(
    store_root: &Path,
    file: &Path,
    actor: &str,
) -> Result<RegistryReport, Error> {
    let profile: ContextProfile = serde_json::from_slice(&fs::read(file)?)?;
    validate_profile(&profile)?;
    register(
        store_root,
        "profiles",
        &profile.id,
        &profile.version,
        &profile,
        actor,
    )
}

pub fn register_selector(
    store_root: &Path,
    file: &Path,
    actor: &str,
) -> Result<RegistryReport, Error> {
    let selector: Selector = serde_json::from_slice(&fs::read(file)?)?;
    validate_selector(&selector)?;
    register(
        store_root,
        "selectors",
        &selector.id,
        &selector.version,
        &selector,
        actor,
    )
}

pub fn load_profile(
    store_root: &Path,
    id: &str,
) -> Result<(ContextProfile, VersionCoordinate), Error> {
    let profile: ContextProfile = load(store_root, "profiles", id)?;
    validate_profile(&profile)?;
    let coordinate = coordinate(&profile.id, &profile.version, &profile)?;
    Ok((profile, coordinate))
}

pub fn load_selector(store_root: &Path, id: &str) -> Result<(Selector, VersionCoordinate), Error> {
    let selector: Selector = load(store_root, "selectors", id)?;
    validate_selector(&selector)?;
    let coordinate = coordinate(&selector.id, &selector.version, &selector)?;
    Ok((selector, coordinate))
}

pub fn profile_files(store_root: &Path) -> Result<Vec<PathBuf>, Error> {
    let mut files = fs::read_dir(store_root.join("registry/profiles"))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|value| value == "json"))
        .collect::<Vec<_>>();
    files.sort();
    Ok(files)
}

fn register<T: Serialize + DeserializeOwned>(
    store_root: &Path,
    area: &str,
    id: &str,
    version: &str,
    value: &T,
    actor: &str,
) -> Result<RegistryReport, Error> {
    let (_guard, _config) = RootGuard::acquire(store_root, actor)?;
    validate_id(id)?;
    let bytes = serde_json::to_vec(value)?;
    let digest = sha256_hex(&bytes);
    let path = registry_path(store_root, area, id)?;
    let _lock = StoreLock::exclusive(store_root)?;
    if path.exists() {
        let existing = fs::read(&path)?;
        let created = existing.strip_suffix(b"\n").unwrap_or(&existing) != bytes;
        if created {
            return Err(Error::Context(format!(
                "{area} entry already registered differently: {id}"
            )));
        }
        return Ok(report(id, version, digest, false));
    }
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    fs::rename(temporary, path)?;
    Ok(report(id, version, digest, true))
}

fn load<T: DeserializeOwned>(store_root: &Path, area: &str, id: &str) -> Result<T, Error> {
    validate_id(id)?;
    let path = registry_path(store_root, area, id)?;
    if !path.is_file() {
        return Err(Error::Context(format!(
            "registered {area} entry not found: {id}"
        )));
    }
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn registry_path(store_root: &Path, area: &str, id: &str) -> Result<PathBuf, Error> {
    validate_id(id)?;
    Ok(store_root
        .join("registry")
        .join(area)
        .join(format!("{id}.json")))
}

fn validate_profile(profile: &ContextProfile) -> Result<(), Error> {
    validate_id(&profile.id)?;
    if profile.version.trim().is_empty()
        || profile.grants.is_empty()
        || profile.selectors.is_empty()
    {
        return Err(Error::Context(
            "profile requires version, grants, and selectors".into(),
        ));
    }
    // Absent bounds are legal and mean "unbounded"; only the values actually
    // present have to agree with each other.
    let budget = &profile.budget;
    if let Some(total) = budget.total {
        let reserve = budget.receipt_reserve();
        if total == 0 {
            return Err(Error::Context(
                "context budget total must be positive".into(),
            ));
        }
        if reserve >= total {
            return Err(Error::Context(format!(
                "receipt_reserve {reserve} leaves no content space in total {total}"
            )));
        }
        let content = total - reserve;
        for (name, value) in [
            ("required_cap", budget.required_cap),
            ("core_cap", budget.core_cap),
            ("relevant_floor", budget.relevant_floor),
        ] {
            if value.is_some_and(|value| value > content) {
                return Err(Error::Context(format!(
                    "{name} exceeds the {content} unit content limit of this budget"
                )));
            }
        }
    }
    Ok(())
}

fn validate_selector(selector: &Selector) -> Result<(), Error> {
    validate_id(&selector.id)?;
    if selector.version.trim().is_empty()
        || selector.type_name.trim().is_empty()
        || selector.strategies.is_empty()
    {
        return Err(Error::Context(
            "selector requires version, type, and strategies".into(),
        ));
    }
    if selector
        .coordinate_pointers
        .values()
        .any(|pointer| !pointer.starts_with('/') || pointer.len() > 500)
    {
        return Err(Error::Context(
            "selector coordinate pointers must be JSON pointers".into(),
        ));
    }
    if selector
        .rank_pointer
        .as_ref()
        .is_some_and(|pointer| !pointer.starts_with('/') || pointer.len() > 500)
    {
        return Err(Error::Context(
            "selector rank pointer must be a JSON pointer".into(),
        ));
    }
    if selector
        .coordinate_modes
        .keys()
        .any(|key| !selector.coordinate_pointers.contains_key(key))
    {
        return Err(Error::Context(
            "selector coordinate modes require matching pointers".into(),
        ));
    }
    Ok(())
}

fn validate_id(id: &str) -> Result<(), Error> {
    if id.is_empty()
        || id.len() > 160
        || !id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
    {
        return Err(Error::Context(
            "registry id must be a stable lowercase identifier".into(),
        ));
    }
    Ok(())
}

fn coordinate<T: Serialize>(
    id: &str,
    version: &str,
    value: &T,
) -> Result<VersionCoordinate, Error> {
    Ok(VersionCoordinate {
        id: id.into(),
        version: version.into(),
        digest: sha256_hex(&serde_json::to_vec(value)?),
    })
}

fn report(id: &str, version: &str, digest: String, created: bool) -> RegistryReport {
    RegistryReport {
        ok: true,
        created,
        id: id.into(),
        version: version.into(),
        digest,
    }
}
