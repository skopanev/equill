use crate::kernel::error::Error;
use crate::kernel::store;
use crate::{context, defense, integrity};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct DoctorReport {
    pub ok: bool,
    pub version: &'static str,
    pub mode: &'static str,
    pub store_initialized: Option<bool>,
    pub checks: Vec<Check>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deep_defense: Option<defense::DeepReport>,
    pub context_profile_faults: usize,
}

#[derive(Debug, Serialize)]
pub struct Check {
    pub id: &'static str,
    pub items: usize,
}

pub fn report(store_root: Option<&Path>, full: bool, deep: bool) -> Result<DoctorReport, Error> {
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
    let mut context_profile_faults = 0;
    if (full || deep)
        && let Some(root) = store_root
    {
        let scan = integrity::scan(root)?;
        context_profile_faults = context::profile_faults(root)?;
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
                id: "context-profile-faults",
                items: context_profile_faults,
            },
            Check {
                id: "projection-files",
                items: scan.projection_files,
            },
            Check {
                id: "projection-records",
                items: scan.projection_records,
            },
            Check {
                id: "import-receipts",
                items: scan.import_receipts,
            },
            Check {
                id: "import-inputs",
                items: scan.import_inputs,
            },
        ]);
    }
    let deep_defense = store_root
        .filter(|_| deep)
        .map(defense::audit)
        .transpose()?;
    let ok = context_profile_faults == 0
        && deep_defense
            .as_ref()
            .is_none_or(|report| report.findings == 0);
    Ok(DoctorReport {
        ok,
        version: env!("CARGO_PKG_VERSION"),
        mode: if deep {
            "deep"
        } else if full {
            "full"
        } else {
            "quick"
        },
        store_initialized,
        checks,
        deep_defense,
        context_profile_faults,
    })
}
