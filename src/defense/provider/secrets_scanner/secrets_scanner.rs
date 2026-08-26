use crate::kernel::error::Error;
use secrets_scanner::{Finding, ScanConfig, Scanner};

#[derive(Debug)]
pub struct Match {
    pub rule: String,
    pub line: usize,
    pub column: usize,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug)]
pub struct Scan {
    pub matches: Vec<Match>,
}

pub fn scan_bundled(content: &str) -> Result<Scan, Error> {
    let scanner = Scanner::from_bundled()
        .map_err(|error| Error::MemoryDefense(format!("bundled rules: {error}")))?
        .with_config(ScanConfig::proxy());
    scan(&scanner, content)
}

pub fn scan_custom(rules: &str, content: &str) -> Result<Scan, Error> {
    let scanner = Scanner::from_toml(rules)
        .map_err(|error| Error::MemoryDefense(format!("store rules: {error}")))?
        .with_config(ScanConfig::proxy());
    scan(&scanner, content)
}

fn scan(scanner: &Scanner, content: &str) -> Result<Scan, Error> {
    let output = scanner
        .scan_proxy(content.as_bytes())
        .map_err(|error| Error::MemoryDefense(format!("scanner rejected input: {error}")))?;
    if output.findings_truncated {
        return Err(Error::MemoryDefense(
            "finding limit reached; refusing an incomplete receipt".into(),
        ));
    }
    Ok(Scan {
        matches: output.findings.into_iter().map(convert).collect(),
    })
}

fn convert(finding: Finding) -> Match {
    Match {
        rule: finding.rule_id,
        line: finding.line,
        column: finding.col,
        start: finding.secret_start_offset,
        end: finding.secret_end_offset,
    }
}
