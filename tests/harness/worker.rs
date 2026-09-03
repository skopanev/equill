//! Observe only the worker owned by one disposable test store.
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

/// Wait until the store's drain lock is free.
pub fn settles(root: &Path, within: Duration) -> bool {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if children(root) == 0 {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// The product is single-flight, so its own lock is a complete 0/1 answer.
pub fn children(root: &Path) -> usize {
    let path = root.join("locks/vector-drain.lock");
    let Ok(file) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
    else {
        return 0;
    };
    if fs2::FileExt::try_lock_exclusive(&file).is_err() {
        return 1;
    }
    let _ = fs2::FileExt::unlock(&file);
    0
}

/// Stop this store's exact worker without scanning any other process.
pub fn kill_worker(root: &Path) -> bool {
    let marker = root.join("projections/qdrant/handoff-active.json");
    let Ok(before) = std::fs::read(&marker) else {
        return false;
    };
    let pid = serde_json::from_slice::<serde_json::Value>(&before)
        .ok()
        .and_then(|claim| claim["pid"].as_u64())
        .filter(|pid| *pid > 0);
    let Some(pid) = pid else {
        return false;
    };
    // Same marker plus held lock narrows the PID-reuse race before the signal.
    if children(root) == 0 || std::fs::read(&marker).ok().as_deref() != Some(before.as_slice()) {
        return false;
    }
    Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status()
        .is_ok_and(|status| status.success())
}
