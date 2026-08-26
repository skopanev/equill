use super::{TypeDefinition, validation};
use crate::kernel::error::Error;
use std::fs;
use std::path::Path;

pub fn verify_all(store_root: &Path) -> Result<usize, Error> {
    let directory = store_root.join("registry/types");
    if !directory.is_dir() {
        return Err(Error::Integrity(format!(
            "required directory is missing: {}",
            directory.display()
        )));
    }
    let mut count = 0;
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        let is_json = path.extension().is_some_and(|value| value == "json");
        if !is_json {
            return Err(Error::Integrity(format!(
                "unexpected schema registry entry: {}",
                path.display()
            )));
        }
        let definition: TypeDefinition = serde_json::from_slice(&fs::read(&path)?)?;
        validation::validate(&definition)?;
        let expected = format!("{}.json", definition.type_name);
        if path.file_name().and_then(|value| value.to_str()) != Some(expected.as_str()) {
            return Err(Error::Integrity(format!(
                "schema filename does not match its type: {}",
                path.display()
            )));
        }
        count += 1;
    }
    Ok(count)
}

pub fn load(store_root: &Path, type_name: &str) -> Result<TypeDefinition, Error> {
    validation::validate_type_name(type_name)?;
    let path = store_root
        .join("registry/types")
        .join(format!("{type_name}.json"));
    if !path.is_file() {
        return Err(Error::InvalidType(format!("{type_name} is not registered")));
    }
    let definition: TypeDefinition = serde_json::from_slice(&fs::read(path)?)?;
    validation::validate(&definition)?;
    if definition.type_name != type_name {
        return Err(Error::Integrity(format!(
            "registered schema does not match requested type: {type_name}"
        )));
    }
    Ok(definition)
}
