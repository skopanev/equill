use crate::kernel::error::Error;
use crate::projection;
use crate::record;
use crate::schema;
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct FullScan {
    pub schemas: usize,
    pub records: usize,
    pub gates: usize,
    pub projection_files: usize,
    pub projection_records: usize,
}

pub fn scan(store_root: &Path) -> Result<FullScan, Error> {
    let stored_records = record::read_all(store_root)?;
    let records = stored_records.len();
    let projection_records = projection::verify(store_root, &stored_records)?;
    Ok(FullScan {
        schemas: schema::verify_all(store_root)?,
        records,
        gates: scan_json_files(&store_root.join("registry/gates"))?,
        projection_files: count_files(&store_root.join("projections"))?,
        projection_records,
    })
}

fn scan_json_files(root: &Path) -> Result<usize, Error> {
    let mut count = 0;
    visit_files(root, &mut |path| {
        if path.extension().is_some_and(|value| value == "json") {
            serde_json::from_slice::<Value>(&fs::read(path)?)
                .map_err(|error| Error::Integrity(format!("{}: {error}", path.display())))?;
            count += 1;
        }
        Ok(())
    })?;
    Ok(count)
}

fn count_files(root: &Path) -> Result<usize, Error> {
    let mut count = 0;
    visit_files(root, &mut |_| {
        count += 1;
        Ok(())
    })?;
    Ok(count)
}

fn visit_files(
    root: &Path,
    visitor: &mut impl FnMut(&Path) -> Result<(), Error>,
) -> Result<(), Error> {
    if !root.is_dir() {
        return Err(Error::Integrity(format!(
            "required directory is missing: {}",
            root.display()
        )));
    }
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_dir() {
            visit_files(&path, visitor)?;
        } else if path.is_file() {
            visitor(&path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::scan;
    use crate::command::init;
    use std::fs;

    #[test]
    fn rejects_broken_record_line() {
        let path = std::env::temp_dir().join(format!("equill-integrity-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        init::create(&path, "test-owner", "agent.memory").expect("initialize");
        fs::write(path.join("records/2026-01.jsonl"), "{broken}\n").expect("fixture");

        let error = scan(&path).expect_err("broken record must fail");

        assert!(error.to_string().contains("2026-01.jsonl:1"));
        fs::remove_dir_all(path).expect("remove test store");
    }
}
