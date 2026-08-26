use crate::integrity;
use crate::kernel::error::Error;
use crate::kernel::store;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct DoctorReport {
    pub ok: bool,
    pub version: &'static str,
    pub mode: &'static str,
    pub store_initialized: Option<bool>,
    pub checks: Vec<Check>,
}

#[derive(Debug, Serialize)]
pub struct Check {
    pub id: &'static str,
    pub items: usize,
}

pub fn report(store_root: Option<&Path>, full: bool) -> Result<DoctorReport, Error> {
    let store_initialized = store_root
        .map(|root| store::load(root).map(|_| true))
        .transpose()?;
    let mut checks = vec![Check {
        id: "executable",
        items: 1,
    }];
    if store_initialized.is_some() {
        checks.push(Check {
            id: "store-metadata",
            items: 1,
        });
    }
    if full {
        if let Some(root) = store_root {
            let scan = integrity::scan(root)?;
            checks.extend([
                Check {
                    id: "schemas",
                    items: scan.schemas,
                },
                Check {
                    id: "records",
                    items: scan.records,
                },
                Check {
                    id: "context-gates",
                    items: scan.gates,
                },
                Check {
                    id: "projection-files",
                    items: scan.projection_files,
                },
            ]);
        }
    }
    Ok(DoctorReport {
        ok: true,
        version: env!("CARGO_PKG_VERSION"),
        mode: if full { "full" } else { "quick" },
        store_initialized,
        checks,
    })
}
