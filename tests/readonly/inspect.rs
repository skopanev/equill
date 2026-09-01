//! Looking at a store closely enough to say nothing moved.
use super::{READER, run};
use std::fs;
use std::path::{Path, PathBuf};

/// The bytes of the ledger and of the WHOLE projection tree.
///
/// The tree, not one file: a catch-up writes the index, its watermark and the
/// published target, and hashing only the database would call three writes out
/// of four "unchanged". Sizes would miss a rewrite that kept the length.
pub fn state(root: &Path) -> (String, String) {
    let ledger = fs::read_dir(root.join("records"))
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| fs::read(entry.path()).unwrap_or_default())
                .fold(Vec::new(), |mut all, mut bytes| {
                    all.append(&mut bytes);
                    all
                })
        })
        .unwrap_or_default();
    (sha256(&ledger), sha256(&tree(&root.join("projections"))))
}

/// What the store answers a reader, so a refused write can be shown to have
/// changed no answer either.
pub fn readback(root: &Path, id: &str) -> (String, String) {
    let one = run(root, READER, &["get", "--id", id]);
    let many = run(root, READER, &["search", "--query", "lesson"]);
    (
        String::from_utf8_lossy(&one.stdout).into_owned(),
        String::from_utf8_lossy(&many.stdout).into_owned(),
    )
}

pub fn sha256(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let digest = <sha2::Sha256 as sha2::Digest>::digest(bytes);
    digest.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

/// Every file under a directory, in a stable order, with its path — so that a
/// file appearing or disappearing changes the hash as surely as an edit does.
fn tree(root: &Path) -> Vec<u8> {
    let mut found = Vec::new();
    collect(root, &mut found);
    found.sort();
    found
        .iter()
        .flat_map(|path| {
            let mut bytes = path.to_string_lossy().into_owned().into_bytes();
            bytes.extend(fs::read(path).unwrap_or_default());
            bytes
        })
        .collect()
}

fn collect(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, found);
        } else {
            found.push(path);
        }
    }
}
