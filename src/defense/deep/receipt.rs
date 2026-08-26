use super::model::{AuditAlert, AuditReceipt, AuditStatus, DeepFinding, DeepReport};
use crate::kernel::error::Error;
use jiff::Timestamp;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

pub fn persist(
    store_root: &Path,
    scan_id: &str,
    corpus_sha256: &str,
    records: usize,
    findings: &[DeepFinding],
) -> Result<DeepReport, Error> {
    let receipt = format!("receipts/defense/{scan_id}.json");
    let alert = (!findings.is_empty()).then(|| format!("alerts/defense/{scan_id}.json"));
    let scanned_at = Timestamp::now().to_string();
    let body = AuditReceipt {
        scan_id,
        status: if findings.is_empty() {
            AuditStatus::Clean
        } else {
            AuditStatus::AttentionRequired
        },
        catalog: "gitleaks-kingfisher-full",
        scanned_at: &scanned_at,
        corpus_sha256,
        records,
        findings,
    };
    write_once(store_root, &receipt, &body)?;
    if let Some(path) = &alert {
        write_once(
            store_root,
            path,
            &AuditAlert {
                scan_id,
                receipt: &receipt,
                findings: findings.len(),
                action: "review affected records and supersede when required",
            },
        )?;
    }
    Ok(DeepReport {
        records,
        findings: findings.len(),
        receipt,
        alert,
    })
}

fn write_once(path: &Path, relative: &str, value: &impl serde::Serialize) -> Result<(), Error> {
    let final_path = path.join(relative);
    if final_path.is_file() {
        return Ok(());
    }
    let directory = final_path
        .parent()
        .ok_or_else(|| Error::Integrity("audit artifact has no parent directory".into()))?;
    fs::create_dir_all(directory)?;
    let temporary = directory.join(format!(".audit-{}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    match fs::rename(&temporary, &final_path) {
        Ok(()) => Ok(()),
        Err(_error) if final_path.is_file() => {
            let _ = fs::remove_file(temporary);
            Ok(())
        }
        Err(error) => {
            let _ = fs::remove_file(temporary);
            Err(error.into())
        }
    }
}
