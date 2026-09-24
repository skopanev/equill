use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn rust_files(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).expect("source directory") {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            rust_files(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
}

#[test]
fn handwritten_rust_respects_file_and_directory_caps() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    rust_files(&root.join("src"), &mut files);
    rust_files(&root.join("tests"), &mut files);
    let mut per_directory = BTreeMap::<PathBuf, usize>::new();
    let mut violations = Vec::new();
    for path in files {
        let source = fs::read_to_string(&path).expect("handwritten Rust source");
        let lines = source.lines().count();
        if lines > 250 {
            violations.push(format!("{} has {lines} lines", path.display()));
        }
        *per_directory
            .entry(path.parent().unwrap().to_owned())
            .or_default() += 1;
    }
    for (directory, count) in per_directory {
        if count > 10 {
            violations.push(format!("{} has {count} Rust files", directory.display()));
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}
